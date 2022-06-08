use crate::{
	auth::{UserTokenClaims, ValidatedAdminToken},
	config::ServerConfig,
	error::ServerError,
	DEVICE_CODE_CHARSET, DEVICE_CODE_LEN,
};
use actix_web::{http, web, HttpResponse};
use chrono::Utc;
use rand::{rngs::OsRng, seq::SliceRandom};
use serde_json::json;
use sqlx::SqlitePool;


pub fn service() -> impl actix_web::dev::HttpServiceFactory {
	web::scope("/admin")
		.service(web::resource("/").to(|| async {
			HttpResponse::Ok()
				.content_type("text/html")
				.body(include_str!("../../admin-webapp/dist/index.html"))
		}))
		.service(web::resource("/main.bundle.js").to(|| async {
			HttpResponse::Ok()
				.content_type("text/javascript")
				.body(include_str!("../../admin-webapp/dist/main.bundle.js"))
		}))
		.service(web::resource("/main.bundle.js.map").to(|| async {
			HttpResponse::Ok()
				.content_type("application/json")
				.body(include_str!("../../admin-webapp/dist/main.bundle.js.map"))
		}))
		.service(web::resource("/pdf.worker.js").to(|| async {
			HttpResponse::Ok()
				.content_type("text/javascript")
				.body(include_str!("../../admin-webapp/dist/pdf.worker.js"))
		}))
		.service(web::resource("/pdf.worker.js.map").to(|| async {
			HttpResponse::Ok()
				.content_type("application/json")
				.body(include_str!("../../admin-webapp/dist/pdf.worker.js.map"))
		}))
		.service(new_device_code)
		.service(new_user_token)
}


#[actix_web::post("/new_device_code")]
async fn new_device_code(_admin_token: ValidatedAdminToken, db_pool: web::Data<SqlitePool>) -> Result<HttpResponse, ServerError> {
	let device_code: Vec<u8> = (0..DEVICE_CODE_LEN)
		.map(|_| *DEVICE_CODE_CHARSET.choose(&mut OsRng).expect("unexpected"))
		.collect();

	sqlx::query("INSERT INTO device_codes (code, date_created) VALUES (?,?)")
		.bind(&device_code)
		.bind(Utc::now().timestamp())
		.execute(&**db_pool)
		.await?;

	Ok(HttpResponse::Ok().json(json!({
		"code": std::str::from_utf8(&device_code).expect("unexpected"),
	})))
}


/// This allows an admin to generate a valid user token that they can then use to interact with the normal API.
#[actix_web::post("/new_user_token")]
async fn new_user_token(admin_token: ValidatedAdminToken, server_config: web::Data<ServerConfig>) -> Result<HttpResponse, ServerError> {
	// Generate a new user token
	let token = UserTokenClaims::admin_new(&admin_token, &server_config);
	Ok(HttpResponse::Ok().insert_header(http::header::ContentType(mime::TEXT_PLAIN)).body(token))
}
