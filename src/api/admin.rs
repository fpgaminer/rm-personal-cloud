use crate::{auth::ValidatedAdminToken, error::ServerError, DEVICE_CODE_CHARSET, DEVICE_CODE_LEN};
use actix_web::{web, HttpResponse, Responder};
use chrono::Utc;
use rand::{rngs::OsRng, seq::SliceRandom};
use serde_json::json;
use sqlx::SqlitePool;


#[actix_web::get("/admin/")]
async fn index() -> impl Responder {
	HttpResponse::Ok().body(
		r###"
	<html>
	<style>
	.device_code {
		font-size: 24px;
		font-family: monospace;
		letter-spacing: .5em;
	}
	</style>
	<body>
		<span class='device_code'></span>
	</body>
	<script>
	fetch('/admin/new_device_code', {
		method: 'post',
		headers: new Headers({
			'Authorization': 'Bearer ' + window.location.hash.substring(1),
		}),
	})
		.then(response => response.json())
		.then(data => {
			document.querySelector('.device_code').textContent = data['code'];
		});
	</script>
	</html>
	"###,
	)
}


#[actix_web::post("/admin/new_device_code")]
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
