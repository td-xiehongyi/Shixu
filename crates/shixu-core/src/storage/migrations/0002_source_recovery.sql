-- Trusted technical source identity and non-secret recovery cursors only.
CREATE TABLE source_bindings (
 source_id TEXT PRIMARY KEY, adapter_type TEXT NOT NULL, account_id TEXT NOT NULL,
 groups_json TEXT NOT NULL, timezone TEXT NOT NULL,
 recovery_epoch INTEGER NOT NULL DEFAULT 0 CHECK(recovery_epoch>=0)
);
CREATE TABLE source_recovery (
 source_id TEXT NOT NULL REFERENCES source_bindings(source_id), group_id TEXT NOT NULL,
 epoch INTEGER NOT NULL CHECK(epoch>0), since INTEGER NOT NULL,
 anchor TEXT, recovery_cursor TEXT, complete INTEGER NOT NULL CHECK(complete IN (0,1)),
 PRIMARY KEY(source_id,group_id)
);
PRAGMA user_version=2;
