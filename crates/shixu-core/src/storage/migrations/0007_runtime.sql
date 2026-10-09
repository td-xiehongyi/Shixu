-- Work references only. No model payload, credentials or reasoning is retained.
CREATE TABLE runtime_work (
 message_key TEXT PRIMARY KEY REFERENCES messages(message_key),
 generation INTEGER NOT NULL DEFAULT 1, pending INTEGER NOT NULL DEFAULT 1,
 model_pending INTEGER NOT NULL DEFAULT 1
);
INSERT INTO runtime_work(message_key) SELECT message_key FROM messages WHERE payload IS NOT NULL;
CREATE TRIGGER runtime_message_insert AFTER INSERT ON messages WHEN NEW.payload IS NOT NULL BEGIN
 INSERT INTO runtime_work(message_key) VALUES(NEW.message_key);
END;
CREATE TRIGGER runtime_message_update AFTER UPDATE OF payload,revision ON messages WHEN NEW.payload IS NOT NULL BEGIN
 INSERT INTO runtime_work(message_key) VALUES(NEW.message_key)
 ON CONFLICT(message_key) DO UPDATE SET generation=generation+1,pending=1,model_pending=1;
END;
CREATE TABLE model_attempts (
 message_key TEXT NOT NULL REFERENCES messages(message_key), revision INTEGER NOT NULL,
 consent_revision INTEGER NOT NULL, consent_session TEXT NOT NULL, source_epoch INTEGER NOT NULL,
 state TEXT NOT NULL CHECK(state IN ('queued','running','done')),
 attempts INTEGER NOT NULL DEFAULT 0 CHECK(attempts BETWEEN 0 AND 3),
 due_at INTEGER NOT NULL, PRIMARY KEY(message_key,revision)
);
PRAGMA user_version=7;
