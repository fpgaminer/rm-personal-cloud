use crate::{
	auth::{FileAccessClaims, ValidatedUserToken},
	config::ServerConfig,
	database,
	error::ServerError,
	notifications::{Notification, NotificationServer},
	FILE_ACCESS_EXPIRATION,
};
use actix_web::{web, HttpResponse};
use chrono::{DateTime, Duration, FixedOffset, NaiveDateTime, Utc};
use log::info;
use serde::Deserialize;
use serde_json::json;
use sqlx::SqlitePool;


#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ListDocumentsQuery {
	doc: Option<String>,
	with_blob: Option<bool>,
}

/// List files
#[actix_web::get("/document-storage/json/2/docs")]
async fn list(
	_user_token: ValidatedUserToken,
	query: web::Query<ListDocumentsQuery>,
	db_pool: web::Data<SqlitePool>,
	server_config: web::Data<ServerConfig>,
) -> Result<HttpResponse, ServerError> {
	let metadata = if let Some(id) = &query.doc {
		if let Some(metadata) = database::get_metadata_by_id(id, &**db_pool).await? {
			vec![metadata]
		} else {
			return Ok(HttpResponse::Ok().json(json!([{
				"ID": id,
				"Message": "Not Found",
				"Success": false,
			}])));
		}
	} else {
		database::list_metadata(&db_pool).await?
	};

	let with_blob = query.with_blob.unwrap_or(false);
	let exp = Utc::now() + Duration::seconds(FILE_ACCESS_EXPIRATION);

	// TODO: What is the response supposed to be when there are no files?  Is it just an empty array?
	let result: Vec<_> = metadata
		.into_iter()
		.map(|x| {
			let blob_url_get = if with_blob {
				let token = FileAccessClaims::new(exp.timestamp(), x.id.clone(), x.version, &server_config);

				format!("https://{}/storage/{}", server_config.server_host, token)
			} else {
				String::new()
			};

			json!({
				"ID": x.id,
				"Version": x.version,
				"ModifiedClient": DateTime::<Utc>::from_utc(NaiveDateTime::from_timestamp(x.client_date_modified, 0), Utc),
				"FileType": x.file_type,
				"VissibleName": x.name, // lol
				"CurrentPage": x.current_page,
				"Bookmarked": x.bookmarked,
				"Parent": x.parent,
				"BlobURLGet": blob_url_get,
				"BlobURLGetExpires": exp,
				"Message": String::new(),
				"Success": true,
			})
		})
		.collect();

	Ok(HttpResponse::Ok().json(result))
}


#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct UploadRequest {
	#[serde(rename = "ID")]
	id: String,
	version: i64,
}

/// Request the upload of a document
/// This API is used during both the creation of a new document and updating an existing document.
#[actix_web::put("/document-storage/json/2/upload/request")]
async fn upload_request(
	_user_token: ValidatedUserToken,
	payload: web::Json<Vec<UploadRequest>>,
	db_pool: web::Data<SqlitePool>,
	server_config: web::Data<ServerConfig>,
) -> Result<HttpResponse, ServerError> {
	// Log request
	info!("payload: {:?}", payload);

	let mut results = Vec::new();

	for req in &*payload {
		// Check version.  If file doesn't exist, version must be 1.  If it already exists, it must be 1 greater than the current version.
		let server_version = database::get_metadata_by_id(&req.id, &**db_pool).await?.map(|x| x.version).unwrap_or(0);

		if req.version != (server_version + 1) {
			results.push(json!({
				"ID": req.id,
				"Version": req.version,
				"Message": format!("Version on server is not -1 of what you supplied: Server: {}, Client req: {}", server_version, req.version),
				"Success": false,
				"BlobURLPut": "",
				"BlobURLPutExpires": "0001-01-01T00:00:00Z",
			}));
			continue;
		}

		let exp = Utc::now() + Duration::seconds(FILE_ACCESS_EXPIRATION);
		let token = FileAccessClaims::new(exp.timestamp(), req.id.clone(), req.version, &server_config);

		results.push(json!({
			"ID": req.id,
			"Version": req.version,
			"Message": "",
			"Success": true,
			"BlobURLPut": format!("https://{}/storage/{}", server_config.server_host, token),
			"BlobURLPutExpires": exp,
		}));
	}

	Ok(HttpResponse::Ok().json(results))
}


/// Upload a file
#[actix_web::put("/storage/{access_token}")]
async fn upload(
	access_token: web::Path<String>,
	body: web::Bytes,
	db_pool: web::Data<SqlitePool>,
	server_config: web::Data<ServerConfig>,
) -> Result<HttpResponse, ServerError> {
	// Authenticate
	let claims = match FileAccessClaims::validate(&access_token, &server_config) {
		Ok(x) => x,
		Err(err) => return Ok(HttpResponse::Unauthorized().body(format!("Bad JWT Token: {:?}", err.into_kind()))),
	};

	// Log request
	info!("upload_document: {:?}", claims);

	// Store in database
	if database::put_data(claims.file_id, claims.file_version, &body, &db_pool).await? == false {
		Ok(HttpResponse::Conflict().body("URL expired"))
	} else {
		Ok(HttpResponse::Ok().finish())
	}
}


/// Download a file
#[actix_web::get("/storage/{access_token}")]
async fn download(
	access_token: web::Path<String>,
	db_pool: web::Data<SqlitePool>,
	server_config: web::Data<ServerConfig>,
) -> Result<HttpResponse, ServerError> {
	// Authenticate
	let claims = match FileAccessClaims::validate(&access_token, &server_config) {
		Ok(x) => x,
		Err(err) => return Ok(HttpResponse::Unauthorized().body(format!("Bad JWT Token: {:?}", err.into_kind()))),
	};

	if let Some(data) = database::get_data_by_id_version(&claims.file_id, claims.file_version, &db_pool).await? {
		Ok(HttpResponse::Ok().body(data))
	} else {
		Ok(HttpResponse::NotFound().body("Not Found"))
	}
}


#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct UpdateRequest {
	#[serde(rename = "ID")]
	id: String,
	version: i64,
	modified_client: DateTime<FixedOffset>, // RFC 3339
	#[serde(rename = "Type")]
	file_type: Option<String>,
	#[serde(rename = "VissibleName")] // lol
	visible_name: Option<String>,
	current_page: Option<i64>,
	bookmarked: Option<bool>,
	parent: Option<String>,
}

/// Update a file's metadata
#[actix_web::put("/document-storage/json/2/upload/update-status")]
async fn update_status(
	user_token: ValidatedUserToken,
	payload: web::Json<Vec<UpdateRequest>>,
	db_pool: web::Data<SqlitePool>,
	notification_server: web::Data<actix::Addr<NotificationServer>>,
) -> Result<HttpResponse, ServerError> {
	// Log request
	info!("payload: {:?}", payload);

	// Now is a good time to clean out old files
	database::clean_deleted_files(&db_pool).await?;

	let mut results = Vec::new();
	let mut notifications = Vec::new();

	let mut tx = database::begin_immediate_transaction(&db_pool).await?;

	for request in &*payload {
		let updated_metadata = database::put_metadata(
			request.id.clone(),
			request.version,
			request.modified_client.timestamp(),
			request.file_type.clone(),
			request.visible_name.clone(),
			request.current_page,
			request.bookmarked,
			request.parent.clone(),
			&mut tx,
		)
		.await?;

		results.push(json!({
			"ID": request.id,
			"Version": request.version,
			"Message": if updated_metadata.is_some() { "" } else { "Version on server is not -1 of what you supplied" },
			"Success": updated_metadata.is_some(),
		}));

		if let Some(updated_metadata) = updated_metadata {
			// Yes, all changes have an event type of "DocAdded"
			notifications.push(Notification::from_metadata(
				"DocAdded",
				&updated_metadata,
				&user_token.0.device_desc,
				&user_token.0.device_id,
			));
		}
	}

	tx.commit().await?;

	for notification in notifications {
		notification.broadcast(&notification_server);
	}

	Ok(HttpResponse::Ok().json(results))
}


#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct DeleteRequest {
	#[serde(rename = "ID")]
	id: String,
	version: i64,
}

/// This API is used to actually delete a file; moving to trash is handled by update-status.
#[actix_web::put("/document-storage/json/2/delete")]
pub async fn delete(
	user_token: ValidatedUserToken,
	payload: web::Json<Vec<DeleteRequest>>,
	db_pool: web::Data<SqlitePool>,
	notification_server: web::Data<actix::Addr<NotificationServer>>,
) -> Result<HttpResponse, ServerError> {
	// Log request
	info!("payload: {:?}", payload);

	let mut results = Vec::new();
	let mut notifications = Vec::new();

	let mut tx = database::begin_immediate_transaction(&db_pool).await?;

	for request in &*payload {
		let old_metadata = database::delete_file(&request.id, request.version, &mut tx).await?;

		results.push(json!({
			"ID": request.id,
			"Version": request.version,
			"Message": if old_metadata.is_some() { "" } else { "Version on server does not match what you supplied" },
			"Success": old_metadata.is_some(),
		}));

		if let Some(old_metadata) = old_metadata {
			notifications.push(Notification::from_metadata(
				"DocDeleted",
				&old_metadata,
				&user_token.0.device_desc,
				&user_token.0.device_id,
			));
		}
	}

	tx.commit().await?;

	for notification in notifications {
		notification.broadcast(&notification_server);
	}

	Ok(HttpResponse::Ok().json(results))
}
