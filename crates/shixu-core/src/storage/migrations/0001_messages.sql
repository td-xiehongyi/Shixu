CREATE TABLE protection_metadata (id INTEGER PRIMARY KEY CHECK(id=1), sentinel BLOB NOT NULL);
CREATE TABLE sources (
 namespace TEXT PRIMARY KEY, source_id TEXT NOT NULL, adapter_type TEXT NOT NULL,
 account_id TEXT NOT NULL, group_id TEXT NOT NULL, cursor TEXT NOT NULL,
 UNIQUE(source_id, adapter_type, account_id, group_id)
);
CREATE TABLE messages (
 message_key TEXT PRIMARY KEY, namespace TEXT NOT NULL REFERENCES sources(namespace),
 native_message_id TEXT NOT NULL, identity_degraded INTEGER NOT NULL CHECK(identity_degraded IN (0,1)),
 revision INTEGER NOT NULL CHECK(revision>0), received_at INTEGER NOT NULL,
 processing_state TEXT NOT NULL CHECK(processing_state IN ('persisted','parsing','committed','retryable_failure','unparseable','non_event','pending','source_revoked')),
 revoked INTEGER NOT NULL CHECK(revoked IN (0,1)),
 payload BLOB, content_digest BLOB NOT NULL,
 UNIQUE(namespace,native_message_id)
);
CREATE INDEX pending_messages ON messages(processing_state,received_at,message_key);
CREATE TABLE part_results (message_key TEXT PRIMARY KEY REFERENCES messages(message_key), revision INTEGER NOT NULL, payload BLOB NOT NULL);
CREATE TABLE suppressions (message_key TEXT PRIMARY KEY REFERENCES messages(message_key), reason TEXT NOT NULL CHECK(reason IN ('user_removed','undo','source_revoked')));
CREATE TABLE source_secrets (source_id TEXT NOT NULL, adapter_type TEXT NOT NULL, account_id TEXT NOT NULL, payload BLOB NOT NULL, PRIMARY KEY(source_id,adapter_type,account_id));
PRAGMA user_version=1;
