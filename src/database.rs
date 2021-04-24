use crate::DELETED_FILE_EXPIRATION;
use anyhow::{Context, Result};
use chrono::Utc;
use sqlx::{sqlite::SqliteRow, Row, SqlitePool};


#[derive(sqlx::FromRow, Default)]
pub struct DbFileMetadata {
	pub id: String,
	pub version: i64,
	pub client_date_modified: i64,
	pub file_type: String,
	pub name: String,
	pub current_page: i64,
	pub bookmarked: bool,
	pub parent: String,
	pub committed: bool,
	pub deleted: i64,
}


pub async fn get_metadata_by_id<'c, E: sqlx::Executor<'c, Database = sqlx::Sqlite>>(id: &str, db: E) -> Result<Option<DbFileMetadata>> {
	sqlx::query_as::<_, DbFileMetadata>("SELECT MAX(version) AS version,id,client_date_modified,file_type,name,current_page,bookmarked,parent,committed,deleted FROM files WHERE id=? AND committed=1 AND deleted=0 GROUP BY id")
		.bind(id)
		.fetch_optional(db)
		.await
		.context("Database")
}

pub async fn get_data_by_id_version(id: &str, version: i64, db: &SqlitePool) -> Result<Option<Vec<u8>>> {
	sqlx::query("SELECT data FROM files WHERE id=? AND version=? AND committed=1 AND deleted=0")
		.bind(id)
		.bind(version)
		.map(|row: SqliteRow| {
			let data: Option<Vec<u8>> = row.get(0);
			data
		})
		.fetch_optional(db)
		.await
		.context("Database")
		.map(|x| x.flatten())
}


pub async fn list_metadata(db: &SqlitePool) -> Result<Vec<DbFileMetadata>> {
	sqlx::query_as::<_, DbFileMetadata>("SELECT MAX(version) AS version,id,client_date_modified,file_type,name,current_page,bookmarked,parent,committed,deleted FROM files WHERE committed=1 AND deleted=0 GROUP BY id")
		.fetch_all(db)
		.await
		.context("Database")
}


/// Returns Ok(true) if the data has been successfully added to the database.
/// Returns Ok(false) when version is not correct.
/// Returns an error for things like Sqlite errors.
pub async fn put_data(id: String, version: i64, data: &[u8], db: &SqlitePool) -> Result<bool> {
	// Start a transaction
	let mut tx = begin_immediate_transaction(db).await?;

	// Find the latest version of the file, even if it isn't committed yet.
	let row = sqlx::query_as::<_, DbFileMetadata>("SELECT MAX(version) AS version,id,client_date_modified,file_type,name,current_page,bookmarked,parent,committed,deleted FROM files WHERE id=? GROUP BY id")
		.bind(&id)
		.fetch_optional(&mut tx)
		.await?;

	// If the file doesn't exist yet we create a "ghost" metadata that allows the rest of this code to work.
	let metadata = row.unwrap_or(DbFileMetadata {
		id: id,
		version: 0,
		file_type: "DocumentType".to_string(),
		committed: true,
		..Default::default()
	});

	if metadata.deleted != 0 {
		return Ok(false);
	}

	// If the most recent record is committed we can only put data on the next version.  If the most recent record isn't committed, we can only put data on that version.
	if (metadata.committed && (version != metadata.version + 1)) || (!metadata.committed && (version != metadata.version)) {
		return Ok(false);
	}

	// If the most recent version isn't committed yet we can update it.
	if !metadata.committed {
		sqlx::query("UPDATE files SET data=?, client_date_modified=? WHERE id=? AND version=?")
			.bind(data)
			.bind(Utc::now().timestamp())
			.bind(&metadata.id)
			.bind(version)
			.execute(&mut tx)
			.await
			.context("Update next version's file data")?;
	}
	// Otherwise we need to create a new uncommitted record.
	else {
		sqlx::query("INSERT INTO files (id,version,client_date_modified,file_type,name,current_page,bookmarked,parent,committed,data,deleted) VALUES (?,?,?,?,?,?,?,?,?,?,?)")
			.bind(&metadata.id)
			.bind(version)
			.bind(Utc::now().timestamp())
			.bind(metadata.file_type)
			.bind(metadata.name)
			.bind(metadata.current_page)
			.bind(metadata.bookmarked)
			.bind(metadata.parent)
			.bind(false)
			.bind(data)
			.bind(0)
			.execute(&mut tx)
			.await
			.context("Insert next version's file data")?;
	}

	// Commit
	tx.commit().await.context("Database TX")?;

	Ok(true)
}


/// Returns Ok(Some(new_metadata)) if the metadata has been successfully updated.
/// Returns Ok(None) on failure (either bad version or some kind of conflict).
/// Returns an error for things like Sqlite errors.
pub async fn put_metadata(
	id: String,
	version: i64,
	client_date_modified: i64,
	file_type: Option<String>,
	name: Option<String>,
	current_page: Option<i64>,
	bookmarked: Option<bool>,
	parent: Option<String>,
	tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
) -> Result<Option<DbFileMetadata>> {
	// Find the latest version of the file, even if it isn't committed yet.
	let row = sqlx::query_as::<_, DbFileMetadata>("SELECT MAX(version) AS version,id,client_date_modified,file_type,name,current_page,bookmarked,parent,committed,deleted FROM files WHERE id=? AND deleted=0 GROUP BY id")
		.bind(&id)
		.fetch_optional(&mut *tx)
		.await.context("Database")?;

	// If the file doesn't exist yet we create a "ghost" metadata that allows the rest of this code to work.
	let mut metadata = row.unwrap_or(DbFileMetadata {
		id: id,
		version: 0,
		file_type: "CollectionType".to_string(),
		committed: true,
		..Default::default()
	});

	if metadata.deleted != 0 {
		return Ok(None);
	}

	// If the most recent record is committed we can only put metadata on the next version.  If the most recent record isn't committed, we can only put metadata on that version.
	if (metadata.committed && (version != metadata.version + 1)) || (!metadata.committed && (version != metadata.version)) {
		return Ok(None);
	}

	// Modify metadata
	metadata.client_date_modified = client_date_modified;
	metadata.file_type = file_type.unwrap_or(metadata.file_type);
	metadata.name = name.unwrap_or(metadata.name);
	metadata.current_page = current_page.unwrap_or(metadata.current_page);
	metadata.bookmarked = bookmarked.unwrap_or(metadata.bookmarked);
	metadata.parent = parent.unwrap_or(metadata.parent);

	// If the most recent version isn't committed yet we can update it.
	if !metadata.committed {
		sqlx::query(
			"UPDATE files SET client_date_modified=?,file_type=?,name=?,current_page=?,bookmarked=?,parent=?,committed=? WHERE id=? AND version=?",
		)
		.bind(metadata.client_date_modified)
		.bind(&metadata.file_type)
		.bind(&metadata.name)
		.bind(metadata.current_page)
		.bind(metadata.bookmarked)
		.bind(&metadata.parent)
		.bind(true)
		.bind(&metadata.id)
		.bind(version)
		.execute(tx)
		.await
		.context("Update next version's metadata")?;
	}
	// Otherwise we need to create a new committed record.
	else {
		let data: (Option<Vec<u8>>,) = if metadata.version == 0 {
			(None,)
		} else {
			sqlx::query_as("SELECT data FROM files WHERE id=? AND version=?")
				.bind(&metadata.id)
				.bind(metadata.version)
				.fetch_one(&mut *tx)
				.await?
		};

		sqlx::query("INSERT INTO files (id,version,client_date_modified,file_type,name,current_page,bookmarked,parent,data,committed,deleted) VALUES (?,?,?,?,?,?,?,?,?,?,?)")
			.bind(&metadata.id)
			.bind(version)
			.bind(metadata.client_date_modified)
			.bind(&metadata.file_type)
			.bind(&metadata.name)
			.bind(metadata.current_page)
			.bind(metadata.bookmarked)
			.bind(&metadata.parent)
			.bind(data.0)
			.bind(true)
			.bind(0)
			.execute(tx)
			.await.context("Insert next version's file metadata")?;
	}

	metadata.version = version;

	Ok(Some(metadata))
}


/// This simulates a BEGIN IMMEDIATE and returns the transaction.
/// It will continuously retry on SQLITE_BUSY errors.
/// sqlx does not support BEGIN IMMEDIATE directly so we simulate it by starting a transaction and then
/// executing a do-nothing UPDATE.  That upgrades the transaction to a write transaction.
pub async fn begin_immediate_transaction(db: &SqlitePool) -> Result<sqlx::Transaction<'_, sqlx::Sqlite>, sqlx::Error> {
	loop {
		let mut tx = db.begin().await?;

		// Do-nothing update; upgrades transaction to a write transaction.
		let result = sqlx::query("UPDATE files SET id='' WHERE FALSE").execute(&mut tx).await;

		match result {
			Ok(_) => return Ok(tx),
			Err(err) if is_sqlite_busy_err(&err) => (), // SQLITE_BUSY, let's try again
			Err(err) => return Err(err),
		}
	}
}


fn is_sqlite_busy_err(err: &sqlx::Error) -> bool {
	// Dig out the SQLITE error code, if there is one
	let err = err
		.as_database_error()
		.and_then(|db_err| db_err.code())
		.and_then(|code| code.parse::<i64>().ok());

	// SQLITE_BUSY if the first 8 bits of code equal 5
	match err {
		Some(code) if (code & 255) == 5 => true,
		_ => false,
	}
}


/// Returns Ok(Some(old metadata)) if the file has been successfully deleted.
/// Returns Ok(None) when version is not correct.
/// Returns an error for things like Sqlite errors.
pub async fn delete_file(id: &str, version: i64, tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>) -> Result<Option<DbFileMetadata>> {
	let server_metadata = get_metadata_by_id(id, &mut *tx).await?;

	if let Some(server_metadata) = server_metadata {
		if server_metadata.version == version {
			sqlx::query("UPDATE files SET deleted=? WHERE id=?")
				.bind(Utc::now().timestamp())
				.bind(id)
				.execute(tx)
				.await?;

			return Ok(Some(server_metadata));
		}
	}

	Ok(None)
}


/// Permanently delete files that were deleted over DELETED_FILE_EXPIRATION seconds ago
pub async fn clean_deleted_files(db: &SqlitePool) -> Result<()> {
	sqlx::query("DELETE FROM files WHERE deleted != 0 AND deleted < ?")
		.bind(Utc::now().timestamp().checked_sub(DELETED_FILE_EXPIRATION).expect("Overflow"))
		.execute(db)
		.await?;

	Ok(())
}
