-- Persist the effective cap accepted for a task. Existing v0.1 jobs used three.
ALTER TABLE attachment_tasks ADD COLUMN retry_limit INTEGER NOT NULL DEFAULT 3 CHECK(retry_limit BETWEEN 0 AND 3);
PRAGMA user_version=4;
