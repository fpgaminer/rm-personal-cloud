use crate::{error::ServerError, REQUEST_LOG_EXPIRATION};
use actix_web::{
	dev::{Service, ServiceRequest, ServiceResponse, Transform},
	web,
	web::BytesMut,
	HttpMessage, HttpRequest, HttpResponse,
};
use chrono::Utc;
use futures::{
	future::{ok, Ready},
	Future, TryStreamExt,
};
use sqlx::SqlitePool;
use std::{
	cell::RefCell,
	pin::Pin,
	rc::Rc,
	task::{Context, Poll},
};


/// Logs unhandled requests to database so we can inspect later and implement them.
pub async fn default_service(req: HttpRequest, body: web::Bytes, db_pool: web::Data<SqlitePool>) -> Result<HttpResponse, ServerError> {
	let headers = req
		.headers()
		.iter()
		.map(|(name, value)| format!("{}: {}", name, value.to_str().unwrap_or("[[[INVALID ASCII]]]")))
		.collect::<Vec<String>>()
		.join("\n");

	sqlx::query("DELETE FROM request_logs WHERE date < ?")
		.bind(Utc::now().timestamp().checked_sub(REQUEST_LOG_EXPIRATION).expect("Overflow"))
		.execute(&**db_pool)
		.await?;

	sqlx::query("INSERT INTO request_logs (date, url, method, request_headers, request_body) VALUES (?,?,?,?,?)")
		.bind(Utc::now().timestamp())
		.bind(req.uri().to_string())
		.bind(req.method().as_str())
		.bind(headers)
		.bind(&*body)
		.execute(&**db_pool)
		.await?;

	Ok(HttpResponse::NotFound().body("Not Found"))
}


/// This is some WIP middleware.  The ultimate goal is to be able to put this program into a debug mode where it logs all request and response data into a database.  But currently it doesn't handle the body properly for websockets, I guess because it waits for the whole request body to be read.
pub struct RequestBodyLogger;

impl<S: 'static, B> Transform<S> for RequestBodyLogger
where
	S: Service<Request = ServiceRequest, Response = ServiceResponse<B>, Error = actix_web::Error>,
	S::Future: 'static,
	B: 'static,
{
	type Request = ServiceRequest;
	type Response = ServiceResponse<B>;
	type Error = actix_web::Error;
	type InitError = ();
	type Transform = RequestBodyLoggerMiddleware<S>;
	type Future = Ready<Result<Self::Transform, Self::InitError>>;

	fn new_transform(&self, service: S) -> Self::Future {
		ok(RequestBodyLoggerMiddleware {
			service: Rc::new(RefCell::new(service)),
		})
	}
}

pub struct RequestBodyLoggerMiddleware<S> {
	service: Rc<RefCell<S>>,
}

impl<S, B> Service for RequestBodyLoggerMiddleware<S>
where
	S: Service<Request = ServiceRequest, Response = ServiceResponse<B>, Error = actix_web::Error> + 'static,
	S::Future: 'static,
	B: 'static,
{
	type Request = ServiceRequest;
	type Response = ServiceResponse<B>;
	type Error = actix_web::Error;
	type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>>>>;

	fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
		self.service.poll_ready(cx)
	}

	fn call(&mut self, mut req: ServiceRequest) -> Self::Future {
		let mut svc = self.service.clone();

		Box::pin(async move {
			let body = req
				.take_payload()
				.try_fold(BytesMut::new(), |mut body, chunk| async move {
					body.extend_from_slice(&chunk);
					Ok(body)
				})
				.await?;

			log::debug!("request uri: {}", req.uri());
			log::debug!("request method: {}", req.method());
			log::debug!("request headers: {:?}", req.headers());
			if body.len() < 8192 {
				log::debug!("request body: {}", hex::encode(&body));
			} else {
				log::debug!("request body (truncated): {}", hex::encode(&body[..8192]));
			}

			let mut payload = actix_http::h1::Payload::create(true).1;
			//let mut payload = Payload::empty();
			payload.unread_data(body.into());
			req.set_payload(payload.into());

			svc.call(req).await
		})
	}
}
