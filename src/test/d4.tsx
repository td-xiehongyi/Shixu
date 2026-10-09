import { act } from "react";
import { createRoot } from "react-dom/client";
import { mainBridge } from "../contracts/bridge";
import type {
  CalendarEvent,
  MessageEnvelope,
  SettingsSnapshot,
} from "../contracts/domain";
export const id = "11111111-1111-4111-8111-111111111111";
export const event: CalendarEvent = {
  event_id: id,
  title: "合成日程",
  kind: "manual",
  time_precision: "date_only",
  local_date: "2026-10-09",
  start_at: null,
  end_at: null,
  timezone: "Asia/Shanghai",
  raw_time_text: "10月9日",
  location: null,
  status: "active",
  revision: "1",
  user_overrides: [],
};
export const settings: SettingsSnapshot = {
  sources: [],
  model: {
    enabled: false,
    provider_id: null,
    allowed_group_ids: [],
    allow_attachment_text: false,
    revision: "0",
  },
  autostart: false,
  transport_supported: false,
  runtime: {
    running: false,
    pending_rules: 0,
    attachment_queue: 0,
    model_queue: 0,
    last_calendar_commit: null,
    last_error: null,
    sources: [],
  },
};
export const notice: MessageEnvelope = {
  calendar_applied: true,
  message_key: id,
  source_id: id,
  account_id: "synthetic",
  group_id: "模拟群",
  native_message_id: "synthetic",
  sent_at: 0,
  received_at: 0,
  sender_id: "synthetic",
  text: "<img src=x onerror=alert(1)>",
  reply_to: null,
  revision: "1",
  revoked: false,
  processing_state: "pending",
  parts: [
    {
      part_id: id,
      message_key: id,
      kind: "image",
      source_file_ref: null,
      original_name: "图片",
      declared_type: null,
      detected_type: null,
      byte_size: null,
      content_hash: null,
      fetch_state: "unavailable",
      parse_state: "download_failed",
      failure_code: "DOWNLOAD_UNAVAILABLE",
      encrypted_blob_ref: null,
      retained_until: null,
    },
  ],
};
export function port(
  extra: Partial<typeof mainBridge> = {},
): typeof mainBridge {
  return {
    ...mainBridge,
    calendarQuery: async () => [event],
    calendarDetails: async () => ({
      origin: "manual",
      history: [{ change_id: id, before: null, after: event, undone: false }],
      sources: [],
    }),
    notificationList: async () => [notice],
    notificationParts: async () => [],
    settingsRead: async () => settings,
    ...extra,
  };
}
export async function render(node: React.ReactNode) {
  (
    globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }
  ).IS_REACT_ACT_ENVIRONMENT = true;
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  await act(async () => root.render(node));
  return {
    host,
    close: async () => {
      await act(async () => root.unmount());
      host.remove();
    },
    click: async (text: string) => {
      const b = [...host.querySelectorAll("button")].find((b) =>
        b.textContent?.includes(text),
      );
      if (!b) throw new Error(`missing ${text}`);
      await act(async () => b.click());
    },
  };
}
