mod api;
mod auth;
mod config;
mod database;
mod error;
mod notifications;
mod request_logger;


use crate::auth::AdminTokenClaims;
use actix::Actor;
use actix_web::{
	middleware::Logger,
	web::{self, Data},
	App, HttpServer,
};
use anyhow::Result;
use clap::Parser;
use config::ServerConfig;
use env_logger::Env;
use log::{error, info};
use notifications::NotificationServer;
use notify::{watcher, DebouncedEvent, RecursiveMode, Watcher};
use rustls::{Certificate, PrivateKey};
use rustls_pemfile::{certs, pkcs8_private_keys};
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use std::{
	fs::File,
	io::BufReader,
	net::{IpAddr, SocketAddr},
	path::PathBuf,
	thread,
	time::Duration,
};


// TODO: Hopefully at some point chrono updates and we can use fancy const fn Duration::seconds() here
/// How long do user JWT tokens live for
const USER_TOKEN_EXPIRATION: u64 = 24 * 60 * 60; // secs
/// How long do file access JWT tokens live for
const FILE_ACCESS_EXPIRATION: i64 = 20 * 60; // secs
/// How long are device codes valid for
const DEVICE_CODE_EXPIRATION: i64 = 5 * 60; // secs
/// How long to keep request logs around
const REQUEST_LOG_EXPIRATION: i64 = 30 * 24 * 60 * 60; // secs
/// How long to keep deleted files around for
const DELETED_FILE_EXPIRATION: i64 = 30 * 24 * 60 * 60; // secs
/// The official API uses this charset: b"abcdefghijklmnopqrstuvwxyz";
const DEVICE_CODE_CHARSET: &[u8] = b"abcdefghjkmnpqrstuvwxyzABCDEFGHJKLMNPQRSTUVWXYZ23456789";
const DEVICE_CODE_LEN: usize = 8;
const MAXIMUM_REQUEST_SIZE: usize = 256 * 1024 * 1024; // bytes
const WEBSOCKET_HEARTBEAT_INTERVAL: Duration = Duration::from_secs(20);
const WEBSOCKET_CLIENT_TIMEOUT: Duration = Duration::from_secs(40);


#[derive(Clone, Debug, Parser)]
#[clap(name = "rm-personal-cloud", version, about, long_about = None)]
struct Opt {
	#[clap(long = "db", value_parser)]
	db_path: PathBuf,

	#[clap(long = "ssl-cert", value_parser)]
	ssl_cert_path: PathBuf,

	#[clap(long = "ssl-key", value_parser)]
	ssl_key_path: PathBuf,

	#[clap(long = "hostname", value_parser, default_value = "local.appspot.com")]
	hostname: String,

	/// Where to listen on (e.g. 0.0.0.0)
	#[clap(long = "bind", value_parser)]
	bind_address: IpAddr,

	#[clap(long = "https-port", default_value = "8084", value_parser)]
	https_port: u16,
}


#[actix_web::main]
async fn main() -> Result<(), anyhow::Error> {
	env_logger::Builder::from_env(Env::default().default_filter_or("warn,actix_web=debug,rm_personal_cloud=debug,actix_server=info")).init();

	let opt = Opt::from_args();

	// Load SSL keys
	let ssl_config = {
		let cert_file = &mut BufReader::new(File::open(&opt.ssl_cert_path).expect("Unable to read SSL cert"));
		let key_file = &mut BufReader::new(File::open(&opt.ssl_key_path).expect("Unable to read SSL key"));

		let cert_chain = certs(cert_file).expect("Invalid SSL cert").into_iter().map(Certificate).collect();
		let mut keys: Vec<PrivateKey> = pkcs8_private_keys(key_file)
			.expect("Invalid SSL key")
			.into_iter()
			.map(PrivateKey)
			.collect();

		rustls::ServerConfig::builder()
			.with_safe_defaults()
			.with_no_client_auth()
			.with_single_cert(cert_chain, keys.remove(0))
			.expect("Invalid SSL key")
	};

	// TODO: Some kind of weird bug in sqlx is causing database open errors for anything more than 2 connections.
	let pool_options = SqlitePoolOptions::new().max_connections(2);
	let db_pool = pool_options
		.connect_with(SqliteConnectOptions::new().filename(opt.db_path).create_if_missing(true))
		.await?;
	sqlx::query(include_str!("../schema.sql")).execute(&db_pool).await?;

	let server_config = ServerConfig::load_config(&db_pool, opt.hostname).await?;
	let notification_server_addr = NotificationServer::new().start();

	println!(
		"Admin URL: https://{}/admin/#{}",
		server_config.server_host,
		AdminTokenClaims::new(&server_config),
	);

	let server = HttpServer::new(move || {
		let logger = Logger::default();

		App::new()
			.wrap(logger)
			.app_data(web::JsonConfig::default().content_type(|_| true)) // The tablet sends some odd content-types for JSON requests, so just accept any
			.app_data(web::PayloadConfig::default().limit(MAXIMUM_REQUEST_SIZE))
			.app_data(Data::new(db_pool.clone()))
			.app_data(Data::new(notification_server_addr.clone()))
			.app_data(Data::new(server_config.clone()))
			.service(api::settings_v1_beta)
			.service(api::v1_reports)
			.service(api::service_discovery)
			.service(api::auth::register_new_device)
			.service(api::auth::device_delete)
			.service(api::auth::new_user_token)
			.service(api::storage::list)
			.service(api::storage::upload_request)
			.service(api::storage::upload)
			.service(api::storage::download)
			.service(api::storage::update_status)
			.service(api::storage::delete)
			.service(notifications::ws_notifications)
			.service(api::admin::service())
			.default_service(web::route().to(request_logger::default_service))
	})
	.bind_rustls(SocketAddr::new(opt.bind_address, opt.https_port), ssl_config)?
	.run();

	cert_watcher(opt.ssl_cert_path.clone(), server.handle());

	server.await?;

	Ok(())
}


/// Watches the SSL certificate file and causes the HttpServer to exit when it changes.
/// We expect some extenral management (e.g. systemd) to restart us, allowing us to reload the cert.
fn cert_watcher(filepath: PathBuf, server: actix_web::dev::ServerHandle) {
	thread::spawn(move || {
		let (tx, rx) = std::sync::mpsc::channel();
		let mut watcher = watcher(tx, Duration::from_secs(10)).expect("Unable to init file watcher");

		watcher
			.watch(filepath, RecursiveMode::NonRecursive)
			.expect("Unable to start file watcher");

		loop {
			match rx.recv() {
				Ok(DebouncedEvent::Write(_)) | Ok(DebouncedEvent::Create(_)) => {
					info!("SSL cert file changed. Exitting so that systemd/docker/etc will restart us.");
					let _ = server.stop(true); // We don't need to await the result
				}
				Ok(x) => info!("Watcher Event: {:?}", x),
				Err(err) => {
					error!("Error while watching cert file: {:?}", err);
					break;
				}
			}
		}
	});
}
