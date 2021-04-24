use crate::{auth::ValidatedUserToken, database::DbFileMetadata, WEBSOCKET_CLIENT_TIMEOUT, WEBSOCKET_HEARTBEAT_INTERVAL};
use actix::prelude::*;
use actix_web::{web, HttpRequest, HttpResponse};
use actix_web_actors::ws;
use log::debug;
use serde_json::json;
use std::time::Instant;


#[actix_web::get("/notifications/ws/json/1")]
pub async fn ws_notifications(
	_user_token: ValidatedUserToken,
	req: HttpRequest,
	stream: web::Payload,
	srv: web::Data<Addr<NotificationServer>>,
) -> Result<HttpResponse, actix_web::Error> {
	ws::start(
		WsNotificationSession {
			last_heartbeat: Instant::now(),
			server_addr: srv.get_ref().clone(),
		},
		&req,
		stream,
	)
}


pub struct Notification(String);

impl Notification {
	pub fn from_metadata(event: &str, metadata: &DbFileMetadata, source_device_desc: &str, source_device_id: &str) -> Self {
		Notification(
			serde_json::to_string(&json!({
				"message": {
					"attributes": {
						"bookmarked": if metadata.bookmarked { "true".to_owned() } else { "false".to_owned() },
						"event": event.to_owned(),
						"id": metadata.id.clone(),
						"parent": metadata.parent.clone(),
						"sourceDeviceDesc": source_device_desc.to_owned(),
						"sourceDeviceID": source_device_id.to_owned(),
						"type": metadata.file_type.clone(),
						"version": metadata.version.to_string(),
						"vissibleName": metadata.name.clone(),
					}
				}
			}))
			.expect("Failed to serialize"),
		)
	}

	pub fn broadcast(self, notification_server: &Addr<NotificationServer>) {
		notification_server.do_send(Message(self.0));
	}
}


struct WsNotificationSession {
	last_heartbeat: Instant,
	/// Address of the NotificationServer actor
	server_addr: Addr<NotificationServer>,
}

impl Actor for WsNotificationSession {
	type Context = ws::WebsocketContext<Self>;

	fn started(&mut self, ctx: &mut Self::Context) {
		// Start heartbeat task
		self.heartbeat(ctx);

		// Send a Subscribe message to the NotificationServer actor so we'll receive notifications
		let my_addr = ctx.address();
		self.server_addr
			.send(Subscribe(my_addr.recipient()))
			.into_actor(self)
			.then(|res, _act, ctx| {
				match res {
					Ok(_) => (),
					_ => ctx.stop(), // Something went wrong
				}
				fut::ready(())
			})
			.wait(ctx);
	}
}

impl Handler<Message> for WsNotificationSession {
	type Result = ();

	fn handle(&mut self, msg: Message, ctx: &mut Self::Context) {
		// Write message to the websocket connection
		ctx.text(msg.0);
	}
}

impl StreamHandler<Result<ws::Message, ws::ProtocolError>> for WsNotificationSession {
	fn handle(&mut self, msg: Result<ws::Message, ws::ProtocolError>, ctx: &mut Self::Context) {
		let msg = match msg {
			Err(_) => {
				ctx.stop();
				return;
			}
			Ok(msg) => msg,
		};

		match msg {
			ws::Message::Ping(msg) => {
				self.last_heartbeat = Instant::now();
				ctx.pong(&msg);
			}
			ws::Message::Pong(_) => {
				self.last_heartbeat = Instant::now();
			}
			ws::Message::Text(_) => (),
			ws::Message::Binary(_) => (),
			ws::Message::Close(reason) => {
				ctx.close(reason);
				ctx.stop();
			}
			ws::Message::Continuation(_) => ctx.stop(),
			ws::Message::Nop => (),
		}
	}
}

// Heartbeat machinery
impl WsNotificationSession {
	fn heartbeat(&self, ctx: &mut ws::WebsocketContext<Self>) {
		ctx.run_interval(WEBSOCKET_HEARTBEAT_INTERVAL, |act, ctx| {
			if Instant::now().duration_since(act.last_heartbeat) > WEBSOCKET_CLIENT_TIMEOUT {
				debug!("Websocket Client heartbeat failed, disconnecting.");

				ctx.stop();
				return;
			}

			ctx.ping(b"");
		});
	}
}


#[derive(Message)]
#[rtype(result = "()")]
struct Message(pub String);

#[derive(Message)]
#[rtype(result = "()")]
struct Subscribe(pub Recipient<Message>);


pub struct NotificationServer {
	subscriptions: Vec<Recipient<Message>>,
}

impl NotificationServer {
	pub fn new() -> Self {
		Self { subscriptions: Vec::new() }
	}

	fn send_message(&mut self, message: &str) {
		self.subscriptions.retain(|addr| {
			// Remove dead subscriptions
			if !addr.connected() {
				debug!("NotificationServer: Removing dead websocket connection");
				return false;
			}

			let _ = addr.do_send(Message(message.to_owned()));
			return true;
		});
	}
}

impl Actor for NotificationServer {
	type Context = Context<Self>;
}

impl Handler<Subscribe> for NotificationServer {
	type Result = ();

	fn handle(&mut self, msg: Subscribe, _: &mut Context<Self>) -> Self::Result {
		debug!("Someone subscribed");

		self.subscriptions.push(msg.0);
		debug!("NotificationServer: Total Connections: {}", self.subscriptions.len());
	}
}

impl Handler<Message> for NotificationServer {
	type Result = ();

	fn handle(&mut self, msg: Message, _: &mut Context<Self>) -> Self::Result {
		debug!("Broadcasting message: {}", msg.0);

		self.send_message(&msg.0);
	}
}
