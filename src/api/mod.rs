pub mod admin;
pub mod auth;
pub mod storage;


use crate::config::ServerConfig;
use actix_web::{web, HttpResponse, Responder};
use serde_json::json;


/// Not entirely sure what this is; I'm guessing the tablet is asking if it has access to beta features?
/// The response is always false, at least for me.
#[actix_web::get("/settings/v1/beta")]
async fn settings_v1_beta() -> impl Responder {
	HttpResponse::Ok().json(json!({"enrolled": false, "available": true}))
}


/// Not entirely sure what this is; I'm guessing the tablet is reporting some analytics or something.
#[actix_web::post("/v1/reports")]
async fn v1_reports(body: web::Bytes) -> impl Responder {
	log::debug!("v1_reports: {}", hex::encode(&body));

	HttpResponse::Ok().finish()
}


/// Service Discovery
/// We always return our hostname, so all connections go to us.
#[actix_web::get("/service/json/1/{service}")]
async fn service_discovery(_service: web::Path<String>, server_config: web::Data<ServerConfig>) -> impl Responder {
	HttpResponse::Ok().json(json!({
		"Host": server_config.server_host,
		"Status": "OK",
	}))
}
