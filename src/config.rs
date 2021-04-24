use actix_web::{web, HttpRequest};
use anyhow::Result;
use rand::{rngs::OsRng, Rng};
use sqlx::SqlitePool;
use std::convert::TryInto;


#[derive(Clone)]
pub struct ServerConfig {
	pub jwt_secret_key: [u8; 32],
	pub server_host: String,
}

impl ServerConfig {
	pub async fn load_config(db: &SqlitePool, server_host: String) -> Result<Self> {
		let jwt_secret_key: [u8; 32] = {
			// Create an encoding key if one doesn't exist
			sqlx::query("INSERT OR IGNORE INTO config (key,value) VALUES (?,?)")
				.bind("jwt_secret_key")
				.bind(hex::encode(OsRng.gen::<[u8; 32]>()))
				.execute(db)
				.await?;

			// Fetch encoding key from database
			let secret: (String,) = sqlx::query_as("SELECT value FROM config WHERE key=?")
				.bind("jwt_secret_key")
				.fetch_one(db)
				.await?;
			let secret = hex::decode(secret.0).expect("Corrupt jwt_secret_key in database.");

			secret.try_into().expect("Corrupt jwt_secret_key in database.")
		};

		Ok(ServerConfig { jwt_secret_key, server_host })
	}

	pub fn from_req(req: &HttpRequest) -> &Self {
		req.app_data::<Self>()
			.or_else(|| req.app_data::<web::Data<Self>>().map(|d| d.as_ref()))
			.expect("Missing ServerConfig")
	}
}
