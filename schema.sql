CREATE TABLE IF NOT EXISTS files (
	id TEXT NOT NULL,
	version INTEGER NOT NULL,
	client_date_modified INTEGER NOT NULL,
	file_type TEXT NOT NULL,
	name TEXT NOT NULL,
	current_page INTEGER NOT NULL,
	bookmarked INTEGER NOT NULL,
	parent TEXT NOT NULL,
	data BLOB,
	committed INTEGER NOT NULL,
	deleted INTEGER NOT NULL
);
CREATE UNIQUE INDEX IF NOT EXISTS idx_files_id_version ON files(id,version);


CREATE TABLE IF NOT EXISTS request_logs (
	date INTEGER NOT NULL,
	url TEXT NOT NULL,
	method TEXT NOT NULL,
	request_headers TEXT NOT NULL,
	request_body BLOB NOT NULL
);


CREATE TABLE IF NOT EXISTS config (
	key TEXT PRIMARY KEY NOT NULL,
	value TEXT NOT NULL
);


CREATE TABLE IF NOT EXISTS device_codes (
	code BLOB NOT NULL,
	date_created INTEGER NOT NULL
);