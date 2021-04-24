use crate::{
	auth::{DeviceTokenClaims, UserTokenClaims, ValidatedDeviceToken},
	config::ServerConfig,
	error::ServerError,
	DEVICE_CODE_EXPIRATION,
};
use actix_web::{http, web, HttpResponse, Responder};
use chrono::Utc;
use log::info;
use serde::Deserialize;
use sqlx::SqlitePool;


#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RegisterNewDeviceRequest {
	code: String,
	device_desc: String,
	#[serde(rename = "deviceID")]
	device_id: String,
}

/// New Device Registration
/// The client provides an 8 letter authentication code, which the user gets from the website.
#[actix_web::post("/token/json/2/device/new")]
async fn register_new_device(
	payload: web::Json<RegisterNewDeviceRequest>,
	server_config: web::Data<ServerConfig>,
	db_pool: web::Data<SqlitePool>,
) -> Result<HttpResponse, ServerError> {
	// Log request
	info!("payload: {:?}", payload);

	// Authenticate using the device code
	sqlx::query("DELETE FROM device_codes WHERE date_created < ?")
		.bind(Utc::now().timestamp().checked_sub(DEVICE_CODE_EXPIRATION).expect("Overflow"))
		.execute(&**db_pool)
		.await?;

	let codes: Vec<(Vec<u8>,)> = sqlx::query_as::<_, (Vec<u8>,)>("SELECT code FROM device_codes")
		.fetch_all(&**db_pool)
		.await?;

	if !codes
		.into_iter()
		.any(|code| ring::constant_time::verify_slices_are_equal(payload.code.as_bytes(), &code.0).is_ok())
	{
		return Ok(HttpResponse::Unauthorized().body("Invalid One-time-code"));
	}

	// Generate a new device token
	let token = DeviceTokenClaims::new_token(payload.device_desc.clone(), payload.device_id.clone(), &server_config);

	Ok(HttpResponse::Ok().header(http::header::CONTENT_TYPE, "text/plain").body(token))
}


/// Delete device
/// We don't really do anything with this, since we don't implement token invalidation.
#[actix_web::post("/token/json/3/device/delete")]
async fn device_delete(_device_token: ValidatedDeviceToken) -> impl Responder {
	HttpResponse::Ok().finish()
}


/// New User Token
/// A client must grab a user token to access the rest of the authenticated APIs.
/// This API is authenticated using the device token.
/// A user token has a short expiration time (24hrs), so the client is expected to refresh it periodically.
#[actix_web::post("/token/json/2/user/new")]
async fn new_user_token(device_token: ValidatedDeviceToken, server_config: web::Data<ServerConfig>) -> Result<HttpResponse, ServerError> {
	// Generate a new user token
	let token = UserTokenClaims::new(&device_token.0, &server_config);

	Ok(HttpResponse::Ok().header(http::header::CONTENT_TYPE, "text/plain").body(token))
}
