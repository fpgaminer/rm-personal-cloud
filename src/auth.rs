use crate::{config::ServerConfig, USER_TOKEN_EXPIRATION};
use actix_web::{dev, error::ErrorUnauthorized, Error, FromRequest, HttpRequest};
use chrono::Utc;
use futures::future;
use jsonwebtoken::{DecodingKey, EncodingKey, Validation};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::json;
use std::convert::TryInto;


pub type ValidatedAdminToken = JWTAuthorization<AdminTokenClaims>;
pub type ValidatedDeviceToken = JWTAuthorization<DeviceTokenClaims>;
pub type ValidatedUserToken = JWTAuthorization<UserTokenClaims>;


#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct AdminTokenClaims {
	sub: String,
}

impl AdminTokenClaims {
	pub fn new(server_config: &ServerConfig) -> String {
		let claims = AdminTokenClaims {
			sub: "Admin Token".to_string(),
		};
		jsonwebtoken::encode(
			&jsonwebtoken::Header::default(),
			&claims,
			&EncodingKey::from_secret(&server_config.jwt_secret_key),
		)
		.expect("Unable to encode JWT")
	}
}

impl JWTValidation for AdminTokenClaims {
	fn validation() -> Validation {
		let mut v = Validation::new(jsonwebtoken::Algorithm::HS256);
		v.validate_exp = false;
		v.set_required_spec_claims(&["sub"]);
		v.sub = Some("Admin Token".to_string());
		v
	}
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct DeviceTokenClaims {
	auth0_userid: String,
	//jti: String,
	device_desc: String,
	device_id: String,
	iat: u64,
	iss: String,
	nbf: u64,
	sub: String,
}

impl DeviceTokenClaims {
	pub fn new_token(device_desc: String, device_id: String, server_config: &ServerConfig) -> String {
		let now: u64 = Utc::now().timestamp().try_into().expect("Cannot support negative timestamps");

		let claims = DeviceTokenClaims {
			auth0_userid: "auth0|325d6aed93e221ecd2f9a277".to_owned(),
			device_desc: device_desc,
			device_id: device_id,
			iat: now,
			iss: "rM WebApp".to_string(),
			nbf: now,
			sub: "rM Device Token".to_string(),
		};

		jsonwebtoken::encode(
			&jsonwebtoken::Header::default(),
			&claims,
			&EncodingKey::from_secret(&server_config.jwt_secret_key),
		)
		.expect("Unable to encode JWT")
	}
}

impl JWTValidation for DeviceTokenClaims {
	fn validation() -> Validation {
		let mut v = Validation::new(jsonwebtoken::Algorithm::HS256);
		v.validate_exp = false;
		v.set_required_spec_claims(&["sub"]);
		v.sub = Some("rM Device Token".to_string());
		v
	}
}


#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct UserTokenClaims {
	pub device_id: String,
	pub device_desc: String,
	exp: u64,
	iat: u64,
	iss: String,
	nbf: u64,
	sub: String,
	auth0_profile: serde_json::Value,
	scopes: String,
}

impl UserTokenClaims {
	pub fn new(device_token: &DeviceTokenClaims, server_config: &ServerConfig) -> String {
		Self::new_from_raw(device_token.device_id.clone(), device_token.device_desc.clone(), server_config)
	}

	pub fn admin_new(_: &ValidatedAdminToken, server_config: &ServerConfig) -> String {
		Self::new_from_raw("admin".to_owned(), "admin".to_owned(), server_config)
	}

	fn new_from_raw(device_id: String, device_desc: String, server_config: &ServerConfig) -> String {
		let now: u64 = Utc::now().timestamp().try_into().expect("Cannot support negative timestamps");
		let exp = now
			.checked_add(USER_TOKEN_EXPIRATION)
			.expect("Unable to represent user token expiration time using a u64");

		let my_claims = UserTokenClaims {
			device_id: device_id,
			device_desc: device_desc,
			exp: exp,
			iat: now,
			iss: "rm-personal-cloud".to_string(),
			nbf: now,
			sub: "rM User Token".to_string(),
			auth0_profile: json!({
				"UserID": "auth0|325d6aed93e221ecd2f9a277",
				"IsSocial": false,
				"Connection": "Username-Password-Authentication",
				"Name": "rm-personal-cloud@example.com",
				"Nickname": "rm-personal-cloud",
				"GivenName": "",
				"FamilyName": "",
				"Email": "rm-personal-cloud@example.com",
				"EmailVerified": true,
			}),
			scopes: "sync:default".to_owned(),
		};

		jsonwebtoken::encode(
			&jsonwebtoken::Header::default(),
			&my_claims,
			&EncodingKey::from_secret(&server_config.jwt_secret_key),
		)
		.expect("Unable to encode JWT")
	}
}

impl JWTValidation for UserTokenClaims {
	fn validation() -> Validation {
		let mut v = Validation::new(jsonwebtoken::Algorithm::HS256);
		v.validate_exp = true;
		v.set_required_spec_claims(&["sub", "exp"]);
		v.sub = Some("rM User Token".to_string());
		v
	}
}


#[derive(Debug, Serialize, Deserialize)]
pub struct FileAccessClaims {
	pub exp: u64,
	pub file_id: String,
	pub file_version: i64,
}

impl JWTValidation for FileAccessClaims {
	fn validation() -> Validation {
		let mut v = Validation::new(jsonwebtoken::Algorithm::HS256);
		v.validate_exp = true;
		v
	}
}

impl FileAccessClaims {
	pub fn new(exp: i64, file_id: String, file_version: i64, server_config: &ServerConfig) -> String {
		let claims = FileAccessClaims {
			exp: exp.try_into().expect("overflow"),
			file_id,
			file_version,
		};

		jsonwebtoken::encode(
			&jsonwebtoken::Header::default(),
			&claims,
			&EncodingKey::from_secret(&server_config.jwt_secret_key),
		)
		.expect("Unable to encode JWT")
	}

	pub fn validate(token: &str, server_config: &ServerConfig) -> Result<Self, jsonwebtoken::errors::Error> {
		let mut validation = Validation::new(jsonwebtoken::Algorithm::HS256);
		validation.validate_exp = true;

		jsonwebtoken::decode(token, &DecodingKey::from_secret(&server_config.jwt_secret_key), &validation).map(|x| x.claims)
	}
}


pub struct JWTAuthorization<T>(pub T);

pub trait JWTValidation {
	fn validation() -> Validation;
}

impl<T: DeserializeOwned + JWTValidation> FromRequest for JWTAuthorization<T> {
	type Error = Error;
	type Future = future::Ready<Result<Self, Error>>;

	fn from_request(req: &HttpRequest, _: &mut dev::Payload) -> Self::Future {
		let server_config = ServerConfig::from_req(req);

		let auth_header = req.headers().get("Authorization").and_then(|auth| auth.to_str().ok());
		let authtoken = match auth_header {
			Some(auth_header) => {
				let mut split = auth_header.split(" ");
				let scheme = split.next().unwrap_or("").to_lowercase();
				if scheme != "bearer" {
					return future::err(ErrorUnauthorized("Expects Bearer Authorization"));
				}

				split.next().unwrap_or("")
			}
			None => return future::err(ErrorUnauthorized("Missing Authorization Header")),
		};

		// Decode and validate the JWT
		let validation = T::validation();
		let authtoken = match jsonwebtoken::decode::<T>(&authtoken, &DecodingKey::from_secret(&server_config.jwt_secret_key), &validation) {
			Ok(authtoken) => authtoken,
			Err(err) => return future::err(ErrorUnauthorized(format!("Bad JWT Token: {:?}", err.into_kind()))),
		};

		future::ok(Self(authtoken.claims))
	}
}
