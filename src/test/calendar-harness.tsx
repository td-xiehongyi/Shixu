// Test-only browser entry; never imported by App or included as a product entry.
import { useState } from "react";
import { createRoot } from "react-dom/client";
import "@fontsource-variable/noto-sans-sc";
import "../styles.css";
import { CalendarPage } from "../features/calendar/CalendarPage";
import { NotificationsPage } from "../features/notifications/NotificationsPage";
import { SettingsPage } from "../features/settings/SettingsPage";
import { event, notice, settings, port } from "./d4";
import type { CalendarEvent, EventChange } from "../contracts/domain";
let events: CalendarEvent[] = [
  event,
  {
    ...event,
    event_id: "33333333-3333-4333-8333-333333333333",
    title: "合成待定活动",
    time_precision: "unknown_date",
    local_date: null,
  },
];
let snapshot = structuredClone(settings);
const changes: EventChange[] = [];
const p = port({
  calendarQuery: async (q) =>
    events.filter((e) => q.statuses.includes(e.status)),
  calendarDetails: async () => ({
    origin: "source",
    history: changes,
    sources: [
      {
        message_key: notice.message_key,
        message_revision: "1",
        group_id: "合成测试群",
        outcome: "applied",
        evidence: [
          {
            part_id: notice.message_key,
            page_or_sheet: { kind: "page", number: 1 },
            cell_range_or_bbox: null,
            text: "<script>untrusted source</script> 2026年10月9日日程",
            method: "native_text",
            engine_version: "synthetic",
            quality_flags: [],
          },
        ],
      },
    ],
  }),
  createManualEvent: async (patch) => {
    const t = patch.time!;
    const e: CalendarEvent = {
      ...event,
      event_id: patch.event_id,
      title: patch.title!,
      time_precision: t.precision,
      local_date: t.local_date,
      start_at: t.start_at,
      end_at: t.end_at,
      timezone: t.timezone,
      raw_time_text: t.raw_time_text,
    };
    events = [...events, e];
    changes.unshift({
      change_id: crypto.randomUUID(),
      before: null,
      after: e,
      undone: false,
    });
    return e;
  },
  calendarEdit: async (id, rev, patch) => {
    const before = events.find((e) => e.event_id === id)!;
    if (before.revision !== rev) throw new Error("CONFLICT");
    const after = {
      ...before,
      title: patch.title ?? before.title,
      revision: String(BigInt(rev) + 1n),
      user_overrides: ["title" as const],
    };
    events = events.map((e) => (e.event_id === id ? after : e));
    changes.unshift({
      change_id: crypto.randomUUID(),
      before,
      after,
      undone: false,
    });
    return after;
  },
  calendarUndo: async (request) => {
    const change = changes.find((c) => c.change_id === request.change_id)!;
    change.undone = true;
    const e = change.before ?? { ...change.after, status: "removed" as const };
    events = events.map((v) => (v.event_id === e.event_id ? e : v));
    return e;
  },
  settingsRead: async () => snapshot,
  saveSourceConfig: async (config) => {
    snapshot = {
      ...snapshot,
      sources: [
        { config, epoch: String(Number(snapshot.sources[0]?.epoch ?? 0) + 1) },
      ],
    };
  },
  setModelConsent: async (model) => {
    snapshot = {
      ...snapshot,
      model: { ...model, revision: String(BigInt(model.revision) + 1n) },
    };
  },
  setAutostart: async (autostart) => {
    snapshot = { ...snapshot, autostart };
  },
});
function Harness() {
  const [page, setPage] = useState("calendar");
  return (
    <div style={{ paddingTop: 60 }}>
      <div
        style={{
          background: "#f7df8d",
          padding: 12,
          position: "fixed",
          top: 0,
          left: 0,
          right: 0,
          zIndex: 20,
        }}
      >
        仅合成界面交互验证 · 不是 QQ、密码、模型或 Windows 实际运行证据{" "}
        <button onClick={() => setPage("calendar")}>日历测试</button>
        <button onClick={() => setPage("notifications")}>通知测试</button>
        <button onClick={() => setPage("settings")}>设置测试</button>
      </div>
      {page === "calendar" ? (
        <CalendarPage port={p} />
      ) : page === "notifications" ? (
        <NotificationsPage port={p} full />
      ) : (
        <SettingsPage port={p} />
      )}
    </div>
  );
}
createRoot(document.getElementById("root")!).render(<Harness />);
