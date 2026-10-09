-- No URLs, credentials, plaintext payloads or filesystem paths in task metadata.
CREATE TABLE attachment_tasks (
 message_key TEXT NOT NULL REFERENCES messages(message_key),
 revision INTEGER NOT NULL CHECK(revision>0), part_id TEXT NOT NULL,
 state TEXT NOT NULL CHECK(state IN ('queued','running','done')),
 retries INTEGER NOT NULL DEFAULT 0 CHECK(retries BETWEEN 0 AND 3),
 due_at INTEGER NOT NULL, lease TEXT,
 PRIMARY KEY(message_key,revision,part_id)
);
CREATE INDEX attachment_due ON attachment_tasks(state,due_at);
PRAGMA user_version=3;
