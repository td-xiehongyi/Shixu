import { describe, expect, it, vi } from "vitest";
import {
  canonicalRevision,
  createMainBridge,
  createVaultBridge,
  decodeEvent,
  decodeSummary,
} from "./bridge";
import fixture from "../../src-tauri/tests/fixtures/event.json";

describe("strict native bridge contract", () => {
  it("main_window_cannot_call_vault", () => {
    const bridge = createMainBridge(async () => undefined);
    expect((bridge as Record<string, unknown>).vaultReveal).toBeUndefined();
    expect(Object.keys(bridge).sort()).toEqual(
      [
        "backupDelete",
        "backupExport",
        "backupImport",
        "backupList",
        "backupPreview",
        "backupPrevious",
        "backupRestore",
        "backupSnapshot",
        "backupVault",
        "createManualEvent",
        "calendarDetails",
        "notificationList",
        "notificationParts",
        "retryPart",
        "settingsRead",
        "saveSourceConfig",
        "setModelConsent",
        "setAutostart",
        "calendarEdit",
        "calendarQuery",
        "calendarUndo",
        "showVaultWindow",
      ].sort(),
    );
  });
  it("dto_roundtrip_matches_rust_fixture", () => {
    expect(decodeEvent(fixture).revision).toBe("9007199254740993");
    expect(JSON.parse(JSON.stringify(decodeEvent(fixture)))).toEqual(fixture);
    expect(() =>
      decodeEvent({ ...fixture, revision: 9007199254740992 }),
    ).toThrow("INVALID_INPUT");
  });
  it("payload_limits_and_revision_are_checked before invoking", async () => {
    let calls = 0;
    const bridge = createMainBridge(async () => {
      calls++;
      return fixture;
    });
    for (const value of [
      "01",
      "+1",
      "-1",
      "1.0",
      " 1",
      "18446744073709551616",
      9007199254740992,
    ]) {
      expect(() => canonicalRevision(value)).toThrow("INVALID_INPUT");
    }
    await expect(
      bridge.calendarEdit(fixture.event_id, "01", {
        event_id: fixture.event_id,
        expected_revision: "01",
        title: null,
        time: null,
        location: null,
        status: null,
      }),
    ).rejects.toThrow("INVALID_INPUT");
    expect(calls).toBe(0);
  });
  it("summary rejects secrets and unsafe timestamps", () => {
    const summary = {
      entry_id: fixture.event_id,
      channel: "演示",
      account: "synthetic",
      revision: "9007199254740993",
      created_at: 0,
      updated_at: 1,
    };
    expect(decodeSummary(summary)).not.toHaveProperty("password");
    expect(() =>
      decodeSummary({ ...summary, password: "synthetic-only" }),
    ).toThrow("INVALID_INPUT");
    expect(() =>
      decodeSummary({ ...summary, updated_at: 9007199254740992 }),
    ).toThrow("INVALID_INPUT");
  });
  it("errors never expose arbitrary native messages", async () => {
    const bridge = createMainBridge(async () => {
      throw "raw-sensitive-native-detail";
    });
    await expect(
      bridge.calendarQuery({
        from_date: null,
        through_date: null,
        statuses: [],
        include_pending: false,
      }),
    ).rejects.toThrow("UNSUPPORTED");
  });
  it("vault mutations reject oversized synthetic bytes before transport", async () => {
    let calls = 0;
    const bridge = createVaultBridge(async () => {
      calls++;
      return null;
    });
    await expect(
      bridge.vaultApply({
        operation: "create",
        channel: "演示",
        account: "synthetic",
        password: new Uint8Array(65537),
      }),
    ).rejects.toThrow("INVALID_INPUT");
    expect(calls).toBe(0);
  });
});

it("strict bridge rejects malformed IDs, nested DTOs and unknown payload fields", async () => {
  let calls = 0;
  const bridge = createMainBridge(async () => {
    calls++;
    return fixture;
  });
  const patch = {
    event_id: fixture.event_id,
    expected_revision: "9007199254740993",
    title: null,
    time: null,
    location: null,
    status: null,
  };
  await expect(
    bridge.calendarEdit("../vault", patch.expected_revision, patch),
  ).rejects.toThrow("INVALID_INPUT");
  await expect(
    bridge.calendarEdit(fixture.event_id, patch.expected_revision, {
      ...patch,
      title: "x".repeat(4097),
    }),
  ).rejects.toThrow("INVALID_INPUT");
  await expect(
    bridge.calendarEdit(fixture.event_id, "1", patch),
  ).rejects.toThrow("INVALID_INPUT");
  await expect(
    bridge.calendarUndo({
      change_id: fixture.event_id,
      expected_revision: "+1",
    }),
  ).rejects.toThrow("INVALID_INPUT");
  await expect(
    bridge.calendarQuery({
      from_date: "2026-02-30",
      through_date: null,
      statuses: [],
      include_pending: false,
    }),
  ).rejects.toThrow("INVALID_INPUT");
  expect(calls).toBe(0);
  expect(() => decodeEvent({ ...fixture, session_id: "forged" })).toThrow(
    "INVALID_INPUT",
  );
});

it("successful command reception preserves every revision above 2^53", async () => {
  const commands: string[] = [];
  const bridge = createMainBridge(async (command) => {
    commands.push(command);
    return command === "calendar_query" ? [fixture] : fixture;
  });
  const patch = {
    event_id: fixture.event_id,
    expected_revision: fixture.revision,
    title: null,
    time: null,
    location: null,
    status: null,
  };
  expect(
    (
      await bridge.calendarQuery({
        from_date: null,
        through_date: null,
        statuses: [],
        include_pending: false,
      })
    )[0].revision,
  ).toBe(fixture.revision);
  expect(
    (await bridge.calendarEdit(fixture.event_id, fixture.revision, patch))
      .revision,
  ).toBe(fixture.revision);
  expect(
    (
      await bridge.calendarUndo({
        change_id: fixture.event_id,
        expected_revision: fixture.revision,
      })
    ).revision,
  ).toBe(fixture.revision);
  expect(commands).toEqual([
    "calendar_query",
    "calendar_edit",
    "calendar_undo",
  ]);
});

it("D3 contracts validate create/change/copy without adding main authority", async () => {
  const sent: {
    command: string;
    payload: Record<string, unknown> | undefined;
  }[] = [];
  const bridge = createVaultBridge(async (command, payload) => {
    sent.push({ command, payload });
    return null;
  });
  await bridge.vaultCreate(Uint8Array.of(7));
  await bridge.vaultChangeMaster(Uint8Array.of(7), Uint8Array.of(8));
  await bridge.vaultCopy(fixture.event_id, "account");
  expect(sent.map((x) => x.command)).toEqual([
    "vault_create",
    "vault_change_master",
    "vault_copy",
  ]);
  expect(sent[2].payload).toEqual({ id: fixture.event_id, field: "account" });
  await expect(bridge.vaultCreate(new Uint8Array())).rejects.toThrow(
    "INVALID_INPUT",
  );
  await expect(
    bridge.vaultChangeMaster(Uint8Array.of(7), new Uint8Array()),
  ).rejects.toThrow("INVALID_INPUT");
  await expect(bridge.vaultCopy("../secret", "password")).rejects.toThrow(
    "INVALID_INPUT",
  );
  await expect(
    bridge.vaultCopy(fixture.event_id, "url" as "account"),
  ).rejects.toThrow("INVALID_INPUT");
  expect(sent.length).toBe(3);
});

it("D3 owned IPC arrays are erased after fulfillment and rejection", async () => {
  let payload: Record<string, unknown> | undefined;
  const bridge = createVaultBridge(async (_command, value) => {
    payload = value;
    throw "UNSUPPORTED";
  });
  await expect(
    bridge.vaultChangeMaster(Uint8Array.of(7), Uint8Array.of(8)),
  ).rejects.toThrow("UNSUPPORTED");
  expect(payload?.current).toEqual([0]);
  expect(payload?.next).toEqual([0]);
  const successful = createVaultBridge(async (_command, value) => {
    payload = value;
    return null;
  });
  await successful.vaultCreate(Uint8Array.of(7));
  expect(payload?.master).toEqual([0]);
});

// D3-R2: incoming mutable IPC bytes are independently owned and cleared after decoding.
it("incoming_reveal_array_is_wiped_after_independent_successful_decode", async () => {
  const incoming = [7, 8, 9];
  const bridge = createVaultBridge(async () => incoming);
  const result = await bridge.vaultReveal(fixture.event_id);
  expect([...result]).toEqual([7, 8, 9]);
  expect(incoming).toEqual([0, 0, 0]);
  result.fill(0);
  expect(incoming).toEqual([0, 0, 0]);
});
it("invalid_incoming_reveal_array_is_wiped_on_rejection", async () => {
  const incoming = [7, 256, 9];
  const bridge = createVaultBridge(async () => incoming);
  await expect(bridge.vaultReveal(fixture.event_id)).rejects.toThrow(
    "INVALID_INPUT",
  );
  expect(incoming).toEqual([0, 0, 0]);
});

it("actual_core_long_source_native_fixture_decodes_without_expanding_limits", async () => {
  const { default: wire } =
    await import("../../src-tauri/tests/fixtures/long-source-event.json");
  expect(
    new TextEncoder().encode(wire.raw_time_text).length,
  ).toBeLessThanOrEqual(4096);
  expect(wire.raw_time_text.endsWith("…")).toBe(true);
  expect(decodeEvent(wire).title).toBeTruthy();
  expect(() =>
    decodeEvent({ ...wire, raw_time_text: "证".repeat(2000) }),
  ).toThrow("INVALID_INPUT");
  const bridge = createMainBridge(async () => [wire]);
  expect(
    await bridge.calendarQuery({
      from_date: null,
      through_date: null,
      statuses: [],
      include_pending: true,
    }),
  ).toHaveLength(1);
});
it("runtime_status_is_bounded_and_payload_free", async () => {
  const { settings } = await import("../test/d4");
  const runtime = {
    running: true,
    pending_rules: 2,
    attachment_queue: 1,
    model_queue: 0,
    last_calendar_commit: null,
    last_error: "STORAGE_FULL",
    sources: [],
  };
  const bridge = createMainBridge(async () => ({ ...settings, runtime }));
  expect((await bridge.settingsRead()).runtime).toEqual(runtime);
  const malformed = createMainBridge(async () => ({
    ...settings,
    runtime: { ...runtime, model_queue: -1 },
  }));
  await expect(malformed.settingsRead()).rejects.toThrow("INVALID_INPUT");
});

it("backup_bridge_rejects_unconfirmed_output_and_unbounded_or_forged_results", async () => {
  const invoke = vi.fn(async () => ({
    created_at: 0,
    events: 2,
    messages: "0",
    present: "0",
    never_fetched: "0",
    cleaned: "0",
    not_migrated: "0",
  }));
  const bridge = createMainBridge(invoke);
  await expect(bridge.backupExport(false, false)).rejects.toThrow(
    "INVALID_INPUT",
  );
  expect(invoke).not.toHaveBeenCalled();
  await expect(bridge.backupRestore("../fake", true)).rejects.toThrow(
    "INVALID_INPUT",
  );
  expect(invoke).not.toHaveBeenCalled();
  await expect(bridge.backupSnapshot()).rejects.toThrow("INVALID_INPUT");
  await expect(
    bridge.backupImport(" ".repeat(20 * 1024 * 1024 + 1)),
  ).rejects.toThrow("INVALID_INPUT");
  expect(invoke).toHaveBeenCalledTimes(1);
});
