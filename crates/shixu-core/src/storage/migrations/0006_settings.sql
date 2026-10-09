CREATE TABLE source_settings (source_id TEXT PRIMARY KEY, epoch INTEGER NOT NULL CHECK(epoch>0 AND typeof(epoch)='integer'), payload BLOB NOT NULL);
CREATE TABLE message_source_proof (message_key TEXT PRIMARY KEY REFERENCES messages(message_key), epoch INTEGER NOT NULL CHECK(epoch>=0 AND typeof(epoch)='integer'), payload BLOB NOT NULL);
CREATE TABLE desktop_settings (id INTEGER PRIMARY KEY CHECK(id=1), autostart INTEGER NOT NULL CHECK(autostart IN (0,1)));
PRAGMA user_version=6;
