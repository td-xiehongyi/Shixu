import { invoke as nativeInvoke, isTauri } from "@tauri-apps/api/core";
import type {
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
    if (new TextEncoder().encode(JSON.stringify(payload)).length > 65536)
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
export function createMainBridge(invoke: Invoke) {
  return Object.freeze({
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
