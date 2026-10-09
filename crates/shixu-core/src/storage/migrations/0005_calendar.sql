-- Business content is protected through Database's native DataProtector.
CREATE TABLE calendar_events (event_id TEXT PRIMARY KEY, payload BLOB NOT NULL);
CREATE TABLE calendar_sources (
 candidate_key TEXT PRIMARY KEY, message_key TEXT NOT NULL REFERENCES messages(message_key),
 event_id TEXT REFERENCES calendar_events(event_id), payload BLOB NOT NULL
);
CREATE INDEX calendar_sources_message ON calendar_sources(message_key);
CREATE TABLE calendar_changes (
 change_id TEXT PRIMARY KEY, event_id TEXT NOT NULL REFERENCES calendar_events(event_id), payload BLOB NOT NULL
);
CREATE TABLE calendar_suppressions (candidate_key TEXT PRIMARY KEY REFERENCES calendar_sources(candidate_key), payload BLOB NOT NULL);
CREATE TABLE calendar_batches (message_key TEXT PRIMARY KEY REFERENCES messages(message_key), payload BLOB NOT NULL);
PRAGMA user_version=5;
