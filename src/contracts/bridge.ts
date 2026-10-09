import { invoke as nativeInvoke, isTauri } from "@tauri-apps/api/core";
import type {
  BackupSummary,
  BackupPreview,
  CalendarDetails,
  MessageEnvelope,
  PartResult,
  SourceConfig,
  ModelConsent,
  SettingsSnapshot,
  AppErrorCode,
  CalendarEvent,
  EventPatch,
  EventQuery,
  UndoRequest,
  VaultMutation,
  VaultSummary,
} from "./domain";
export type Invoke = (
  command: string,
  payload?: Record<string, unknown>,
) => Promise<unknown>;
const codes: readonly string[] = [
  "LOCKED",
  "AUTH_FAILED",
  "UNSUPPORTED",
  "CONFLICT",
  "STORAGE_FULL",
  "DISCONNECTED",
  "PARSE_FAILED",
  "INVALID_INPUT",
];
export class BridgeError extends Error {
  constructor(readonly code: AppErrorCode) {
    super(code);
  }
}
const invalid = (): never => {
  throw new BridgeError("INVALID_INPUT");
};
function object(
  value: unknown,
  keys: readonly string[],
): Record<string, unknown> {
  if (!value || typeof value !== "object" || Array.isArray(value))
    return invalid();
  const data = value as Record<string, unknown>;
  if (
    Object.keys(data).some((key) => !keys.includes(key)) ||
    keys.some((key) => !(key in data))
  )
    return invalid();
  return data;
}
export function canonicalRevision(value: unknown): string {
  if (
    typeof value !== "string" ||
    !/^(0|[1-9][0-9]{0,19})$/.test(value) ||
    BigInt(value) > 18446744073709551615n
  )
    return invalid();
  return value;
}
function id(value: unknown): string {
  if (
    typeof value !== "string" ||
    !/^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i.test(
      value,
    )
  )
    return invalid();
  return value;
}
function text(value: unknown, max = 4096): string {
  if (
    typeof value !== "string" ||
    new TextEncoder().encode(value).length > max ||
    value.includes("\0")
  )
    return invalid();
  return value;
}
function millis(value: unknown): void {
  if (!Number.isSafeInteger(value)) invalid();
}
function nullable(value: unknown, check: (value: unknown) => unknown): void {
  if (value !== null) check(value);
}
function enumeration(value: unknown, allowed: readonly string[]): void {
  if (typeof value !== "string" || !allowed.includes(value)) invalid();
}
const statuses = ["active", "cancelled", "removed"];
const precisions = ["unknown_date", "date_only", "exact", "explicit_all_day"];
function date(value: unknown): void {
  const s = text(value, 10);
  const d = new Date(`${s}T00:00:00Z`);
  if (
    !/^\d{4}-\d{2}-\d{2}$/.test(s) ||
    Number.isNaN(d.getTime()) ||
    d.toISOString().slice(0, 10) !== s
  )
    invalid();
}
function time(value: unknown): void {
  const d = object(value, [
    "precision",
    "local_date",
    "start_at",
    "end_at",
    "timezone",
    "raw_time_text",
  ]);
  enumeration(d.precision, precisions);
  nullable(d.local_date, date);
  nullable(d.start_at, millis);
  nullable(d.end_at, millis);
  text(d.timezone, 128);
  text(d.raw_time_text);
}
export function decodeEvent(value: unknown): CalendarEvent {
  const d = object(value, [
    "event_id",
    "title",
    "kind",
    "time_precision",
    "local_date",
    "start_at",
    "end_at",
    "timezone",
    "raw_time_text",
    "location",
    "status",
    "revision",
    "user_overrides",
  ]);
  id(d.event_id);
  text(d.title);
  text(d.kind, 128);
  enumeration(d.time_precision, precisions);
  nullable(d.local_date, date);
  nullable(d.start_at, millis);
  nullable(d.end_at, millis);
  text(d.timezone, 128);
  text(d.raw_time_text);
  nullable(d.location, text);
  enumeration(d.status, statuses);
  canonicalRevision(d.revision);
  if (!Array.isArray(d.user_overrides) || d.user_overrides.length > 4)
    return invalid();
  for (const field of d.user_overrides)
    enumeration(field, ["title", "time", "location", "status"]);
  return d as unknown as CalendarEvent;
}
export function decodeSummary(value: unknown): VaultSummary {
  const d = object(value, [
    "entry_id",
    "channel",
    "account",
    "revision",
    "created_at",
    "updated_at",
  ]);
  id(d.entry_id);
  text(d.channel);
  text(d.account);
  canonicalRevision(d.revision);
  millis(d.created_at);
  millis(d.updated_at);
  return d as unknown as VaultSummary;
}
function query(value: unknown): void {
  const d = object(value, [
    "from_date",
    "through_date",
    "statuses",
    "include_pending",
  ]);
  nullable(d.from_date, date);
  nullable(d.through_date, date);
  if (
    !Array.isArray(d.statuses) ||
    d.statuses.length > 3 ||
    typeof d.include_pending !== "boolean"
  )
    return invalid();
  for (const status of d.statuses) enumeration(status, statuses);
}
function patch(value: unknown): EventPatch {
  const d = object(value, [
    "event_id",
    "expected_revision",
    "title",
    "time",
    "location",
    "status",
  ]);
  id(d.event_id);
  canonicalRevision(d.expected_revision);
  nullable(d.title, text);
  nullable(d.time, time);
  nullable(d.status, (v) => enumeration(v, statuses));
  if (d.location !== null) {
    const l = d.location as Record<string, unknown>;
    if (l?.operation === "clear") object(l, ["operation"]);
    else {
      object(l, ["operation", "value"]);
      if (l.operation !== "set") invalid();
      text(l.value);
    }
  }
  return d as unknown as EventPatch;
}
function bytes(value: unknown): Uint8Array {
  if (!(value instanceof Uint8Array) || !value.length || value.length > 65536)
    return invalid();
  return value;
}
function mutation(value: VaultMutation): Record<string, unknown> {
  const keys =
    value?.operation === "delete"
      ? ["operation", "id", "expected_revision"]
      : value?.operation === "update"
        ? [
            "operation",
            "id",
            "expected_revision",
            "channel",
            "account",
            "password",
          ]
        : ["operation", "channel", "account", "password"];
  const d = object(value, keys);
  enumeration(d.operation, ["create", "update", "delete"]);
  if (d.operation !== "create") {
    id(d.id);
    canonicalRevision(d.expected_revision);
  }
  if (d.operation !== "delete") {
    text(d.channel);
    text(d.account);
    return { ...d, password: Array.from(bytes(d.password)) };
  }
  return d;
}
async function call(
  invoke: Invoke,
  command: string,
  payload: Record<string, unknown> = {},
): Promise<unknown> {
  try {
    if (
      command !== "backup_import" &&
      new TextEncoder().encode(JSON.stringify(payload)).length > 65536
    )
      invalid();
    return await invoke(command, payload);
  } catch (error) {
    if (error instanceof BridgeError) throw error;
    throw new BridgeError(
      typeof error === "string" && codes.includes(error)
        ? (error as AppErrorCode)
        : "UNSUPPORTED",
    );
  }
}
/** Best-effort erasure of owned numeric IPC arrays after native settlement. */
async function secretCall(
  invoke: Invoke,
  command: string,
  payload: Record<string, unknown>,
): Promise<unknown> {
  try {
    return await call(invoke, command, payload);
  } finally {
    const wipe = (value: unknown): void => {
      if (Array.isArray(value)) value.fill(0);
      else if (value && typeof value === "object")
        Object.values(value).forEach(wipe);
    };
    wipe(payload);
  }
}

function bool(v: unknown): void {
  if (typeof v !== "boolean") invalid();
}
function array(v: unknown, max: number, check: (v: unknown) => unknown): void {
  if (!Array.isArray(v) || v.length > max) invalid();
  for (const item of v as unknown[]) check(item);
}
function groups(v: unknown): void {
  array(v, 100, (g) => text(g, 128));
}
function source(v: unknown): void {
  const d = object(v, [
    "source_id",
    "adapter_type",
    "account_id",
    "allowed_group_ids",
    "timezone",
    "enabled",
    "capability_set",
  ]);
  id(d.source_id);
  text(d.adapter_type, 128);
  text(d.account_id, 128);
  groups(d.allowed_group_ids);
  text(d.timezone, 128);
  bool(d.enabled);
  array(d.capability_set, 6, (c) =>
    enumeration(c, [
      "live_messages",
      "backfill",
      "attachments",
      "replies",
      "revocations",
      "edits",
    ]),
  );
}
function consent(v: unknown): void {
  const d = object(v, [
    "enabled",
    "provider_id",
    "allowed_group_ids",
    "allow_attachment_text",
    "revision",
  ]);
  bool(d.enabled);
  nullable(d.provider_id, (p) => text(p, 128));
  groups(d.allowed_group_ids);
  bool(d.allow_attachment_text);
  canonicalRevision(d.revision);
}
const partStates = [
  "pending_download",
  "downloading",
  "fetched",
  "parsing",
  "success",
  "download_failed",
  "unsupported",
  "limit_exceeded",
  "recognition_failed",
  "partial_parse",
];
const reasons = [
  "DOWNLOAD_UNAVAILABLE",
  "FORMAT_UNSUPPORTED",
  "LIMIT_EXCEEDED",
  "RECOGNITION_FAILED",
  "PARTIAL_SOURCE",
  "TIMED_OUT",
  "MEMORY_LIMIT",
  "AUTH_REQUIRED",
  "EXPIRED",
  "PERMISSION_DENIED",
  "STORAGE_FULL",
];
function evidence(v: unknown): void {
  const d = object(v, [
    "part_id",
    "page_or_sheet",
    "cell_range_or_bbox",
    "text",
    "method",
    "engine_version",
    "quality_flags",
  ]);
  id(d.part_id);
  text(d.text);
  text(d.engine_version, 128);
  enumeration(d.method, ["native_text", "ocr", "cell"]);
  array(d.quality_flags, 4, (f) =>
    enumeration(f, [
      "uncertain_date",
      "ambiguous_layout",
      "formula_derived",
      "partial_source",
    ]),
  );
  if (d.page_or_sheet !== null) {
    const p = d.page_or_sheet as Record<string, unknown>;
    if (p.kind === "sheet") {
      object(p, ["kind", "name"]);
      text(p.name, 128);
    } else {
      object(p, ["kind", "number"]);
      enumeration(p.kind, ["page", "paragraph"]);
      if (!Number.isSafeInteger(p.number) || (p.number as number) < 0)
        invalid();
    }
  }
  if (d.cell_range_or_bbox !== null) {
    const p = d.cell_range_or_bbox as Record<string, unknown>;
    if (p.kind === "cell_range") {
      object(p, ["kind", "range"]);
      text(p.range, 128);
    } else {
      const keys =
        p.kind === "text_span"
          ? ["start", "end"]
          : ["x", "y", "width", "height"];
      object(p, ["kind", ...keys]);
      enumeration(p.kind, ["text_span", "bounding_box"]);
      for (const key of keys)
        if (
          typeof p[key] !== "number" ||
          !Number.isFinite(p[key]) ||
          (p[key] as number) < 0
        )
          invalid();
    }
  }
}
function partResult(v: unknown): void {
  const p = object(v, ["part_id", "status", "blocks", "reason_code"]);
  id(p.part_id);
  enumeration(p.status, partStates);
  array(p.blocks, 32, evidence);
  nullable(p.reason_code, (r) => enumeration(r, reasons));
}
function message(v: unknown): MessageEnvelope {
  const d = object(v, [
    "calendar_applied",
    "message_key",
    "source_id",
    "account_id",
    "group_id",
    "native_message_id",
    "sent_at",
    "received_at",
    "sender_id",
    "text",
    "reply_to",
    "revision",
    "revoked",
    "processing_state",
    "parts",
  ]);
  bool(d.calendar_applied);
  id(d.message_key);
  id(d.source_id);
  for (const k of ["account_id", "group_id", "native_message_id", "sender_id"])
    text(d[k], 128);
  text(d.text);
  millis(d.sent_at);
  millis(d.received_at);
  nullable(d.reply_to, id);
  canonicalRevision(d.revision);
  bool(d.revoked);
  enumeration(d.processing_state, [
    "persisted",
    "parsing",
    "committed",
    "retryable_failure",
    "unparseable",
    "non_event",
    "pending",
    "source_revoked",
  ]);
  array(d.parts, 5, (v) => {
    const p = object(v, [
      "part_id",
      "message_key",
      "kind",
      "source_file_ref",
      "original_name",
      "declared_type",
      "detected_type",
      "byte_size",
      "content_hash",
      "fetch_state",
      "parse_state",
      "failure_code",
      "encrypted_blob_ref",
      "retained_until",
    ]);
    id(p.part_id);
    id(p.message_key);
    enumeration(p.kind, ["text", "image", "file"]);
    for (const k of ["source_file_ref", "content_hash", "encrypted_blob_ref"])
      if (p[k] !== null) invalid();
    nullable(p.original_name, (n) => text(n, 256));
    for (const k of ["declared_type", "detected_type"])
      nullable(p[k], (n) => text(n, 128));
    nullable(p.byte_size, canonicalRevision);
    nullable(p.retained_until, millis);
    enumeration(p.fetch_state, [
      "pending",
      "fetching",
      "fetched",
      "unavailable",
    ]);
    enumeration(p.parse_state, partStates);
    nullable(p.failure_code, (r) => enumeration(r, reasons));
  });
  return d as unknown as MessageEnvelope;
}
function details(v: unknown): CalendarDetails {
  const d = object(v, ["origin", "history", "sources"]);
  enumeration(d.origin, ["manual", "source"]);
  array(d.history, 100, (v) => {
    const h = object(v, ["change_id", "before", "after", "undone"]);
    id(h.change_id);
    nullable(h.before, decodeEvent);
    decodeEvent(h.after);
    bool(h.undone);
  });
  array(d.sources, 100, (v) => {
    const s = object(v, [
      "message_key",
      "message_revision",
      "group_id",
      "outcome",
      "evidence",
    ]);
    id(s.message_key);
    canonicalRevision(s.message_revision);
    text(s.group_id, 128);
    enumeration(s.outcome, [
      "applied",
      "pending",
      "conflict",
      "suppressed",
      "revoked",
    ]);
    array(s.evidence, 32, evidence);
  });
  return d as unknown as CalendarDetails;
}
function settings(v: unknown): SettingsSnapshot {
  const d = object(v, [
    "sources",
    "model",
    "autostart",
    "transport_supported",
    "runtime",
  ]);
  const r = object(d.runtime, [
    "running",
    "pending_rules",
    "attachment_queue",
    "model_queue",
    "last_calendar_commit",
    "last_error",
    "sources",
  ]);
  bool(r.running);
  nullable(r.last_calendar_commit, millis);
  nullable(r.last_error, (v) => enumeration(v, codes));
  for (const key of ["pending_rules", "attachment_queue", "model_queue"]) {
    if (
      !Number.isSafeInteger(r[key]) ||
      (r[key] as number) < 0 ||
      (r[key] as number) > 4294967295
    )
      invalid();
  }
  array(r.sources, 100, (v) => {
    const s = object(v, [
      "source_id",
      "connection_state",
      "last_received_at",
      "last_persisted_at",
      "last_applied_at",
      "gap",
    ]);
    id(s.source_id);
    enumeration(s.connection_state, [
      "connected",
      "disconnected",
      "waiting_for_login",
      "incompatible",
    ]);
    bool(s.gap);
    for (const key of [
      "last_received_at",
      "last_persisted_at",
      "last_applied_at",
    ])
      nullable(s[key], millis);
  });
  array(d.sources, 100, (v) => {
    const row = object(v, ["config", "epoch"]);
    source(row.config);
    canonicalRevision(row.epoch);
  });
  consent(d.model);
  bool(d.autostart);
  bool(d.transport_supported);
  return d as unknown as SettingsSnapshot;
}
function backupSummary(
  value: unknown,
  preview = false,
): BackupSummary | BackupPreview {
  const d = object(value, [
    "created_at",
    "events",
    "messages",
    "present",
    "never_fetched",
    "cleaned",
    "not_migrated",
    ...(preview ? ["preview_id"] : []),
  ]);
  millis(d.created_at);
  for (const key of [
    "events",
    "messages",
    "present",
    "never_fetched",
    "cleaned",
    "not_migrated",
  ])
    canonicalRevision(d[key]);
  if (preview) id(d.preview_id);
  return d as unknown as BackupSummary | BackupPreview;
}
export function createMainBridge(invoke: Invoke) {
  return Object.freeze({
    async backupList(): Promise<BackupSummary[]> {
      const r = await call(invoke, "backup_list");
      array(r, 7, backupSummary);
      return r as BackupSummary[];
    },
    async backupSnapshot(): Promise<BackupSummary> {
      return backupSummary(await call(invoke, "backup_snapshot"));
    },
    async backupPreview(day: number): Promise<BackupPreview> {
      if (!Number.isSafeInteger(day)) invalid();
      return backupSummary(
        await call(invoke, "backup_preview", { day }),
        true,
      ) as BackupPreview;
    },
    async backupPrevious(): Promise<BackupPreview> {
      return backupSummary(
        await call(invoke, "backup_previous"),
        true,
      ) as BackupPreview;
    },
    async backupRestore(previewId: string, confirmed: boolean): Promise<void> {
      id(previewId);
      bool(confirmed);
      if (!confirmed) invalid();
      await call(invoke, "backup_restore", {
        preview_id: previewId,
        confirmed,
      });
    },
    async backupDelete(day: number): Promise<void> {
      if (!Number.isSafeInteger(day)) invalid();
      await call(invoke, "backup_delete", { day });
    },
    async backupExport(
      includeRawMessages = false,
      confirmed = false,
    ): Promise<string> {
      bool(includeRawMessages);
      bool(confirmed);
      if (!confirmed) invalid();
      return text(
        await call(invoke, "backup_export", {
          include_raw_messages: includeRawMessages,
          confirmed,
        }),
        20 * 1024 * 1024,
      );
    },
    async backupImport(data: string): Promise<BackupPreview> {
      text(data, 20 * 1024 * 1024);
      return backupSummary(
        await call(invoke, "backup_import", { data }),
        true,
      ) as BackupPreview;
    },
    async backupVault(): Promise<BackupSummary> {
      return backupSummary(await call(invoke, "backup_vault"));
    },
    async createManualEvent(value: EventPatch): Promise<CalendarEvent> {
      patch(value);
      return decodeEvent(
        await call(invoke, "calendar_create_manual", { patch: value }),
      );
    },
    async calendarDetails(eventId: string): Promise<CalendarDetails> {
      id(eventId);
      return details(await call(invoke, "calendar_details", { id: eventId }));
    },
    async notificationList(sourceId?: string): Promise<MessageEnvelope[]> {
      if (sourceId !== undefined) id(sourceId);
      const r = await call(invoke, "notification_list", {
        source_id: sourceId ?? null,
      });
      array(r, 100, message);
      return r as MessageEnvelope[];
    },
    async notificationParts(messageKey: string): Promise<PartResult[]> {
      id(messageKey);
      const r = await call(invoke, "notification_parts", {
        message_key: messageKey,
      });
      array(r, 5, partResult);
      return r as PartResult[];
    },
    async retryPart(partId: string): Promise<void> {
      id(partId);
      await call(invoke, "retry_part", { part_id: partId });
    },
    async settingsRead(): Promise<SettingsSnapshot> {
      return settings(await call(invoke, "settings_read"));
    },
    async saveSourceConfig(value: SourceConfig): Promise<void> {
      source(value);
      await call(invoke, "save_source_config", { config: value });
    },
    async setModelConsent(value: ModelConsent): Promise<void> {
      consent(value);
      await call(invoke, "set_model_consent", { consent: value });
    },
    async setAutostart(enabled: boolean): Promise<void> {
      bool(enabled);
      await call(invoke, "set_autostart", { enabled });
    },
    async calendarQuery(value: EventQuery): Promise<CalendarEvent[]> {
      query(value);
      const result = await call(invoke, "calendar_query", { query: value });
      if (!Array.isArray(result) || result.length > 10000) return invalid();
      return result.map(decodeEvent);
    },
    async calendarEdit(
      eventId: string,
      revision: string,
      value: EventPatch,
    ): Promise<CalendarEvent> {
      id(eventId);
      canonicalRevision(revision);
      patch(value);
      if (eventId !== value.event_id || revision !== value.expected_revision)
        invalid();
      return decodeEvent(
        await call(invoke, "calendar_edit", {
          id: eventId,
          revision,
          patch: value,
        }),
      );
    },
    async calendarUndo(value: UndoRequest): Promise<CalendarEvent> {
      const d = object(value, ["change_id", "expected_revision"]);
      id(d.change_id);
      canonicalRevision(d.expected_revision);
      return decodeEvent(
        await call(invoke, "calendar_undo", { request: value }),
      );
    },
    async showVaultWindow(): Promise<void> {
      await call(invoke, "show_vault_window");
    },
  });
}
/** Only import/use this in the isolated vault view. Native policy remains the authority. */
export function createVaultBridge(invoke: Invoke) {
  return Object.freeze({
    async vaultCreate(master: Uint8Array): Promise<void> {
      await secretCall(invoke, "vault_create", {
        master: Array.from(bytes(master)),
      });
    },
    async vaultChangeMaster(
      current: Uint8Array,
      next: Uint8Array,
    ): Promise<void> {
      await secretCall(invoke, "vault_change_master", {
        current: Array.from(bytes(current)),
        next: Array.from(bytes(next)),
      });
    },
    async vaultCopy(
      entryId: string,
      field: "account" | "password",
    ): Promise<void> {
      id(entryId);
      enumeration(field, ["account", "password"]);
      await call(invoke, "vault_copy", { id: entryId, field });
    },
    async vaultUnlock(master: Uint8Array): Promise<void> {
      await secretCall(invoke, "vault_unlock", {
        master: Array.from(bytes(master)),
      });
    },
    async vaultList(): Promise<VaultSummary[]> {
      const r = await call(invoke, "vault_list");
      if (!Array.isArray(r) || r.length > 10000) return invalid();
      return r.map(decodeSummary);
    },
    async vaultApply(value: VaultMutation): Promise<VaultSummary> {
      return decodeSummary(
        await secretCall(invoke, "vault_apply", { mutation: mutation(value) }),
      );
    },
    async vaultReveal(entryId: string): Promise<Uint8Array> {
      id(entryId);
      const r = await call(invoke, "vault_reveal", { id: entryId });
      try {
        if (
          !Array.isArray(r) ||
          !r.length ||
          r.length > 65536 ||
          r.some((b) => !Number.isInteger(b) || b < 0 || b > 255)
        )
          return invalid();
        return Uint8Array.from(r);
      } finally {
        // We own this mutable IPC array, separately from the returned typed copy.
        // Immutable/inaccessible transport copies remain outside JS erasure control.
        if (Array.isArray(r)) {
          try {
            Array.prototype.fill.call(r, 0);
          } catch {
            /* Best effort for read-only values. */
          }
        }
      }
    },
    async vaultLock(): Promise<void> {
      await call(invoke, "vault_lock");
    },
  });
}
export const desktopInvoke: Invoke = async (command, payload) => {
  if (!isTauri()) throw "UNSUPPORTED";
  return nativeInvoke(command, payload);
};
export const mainBridge = createMainBridge(desktopInvoke);
