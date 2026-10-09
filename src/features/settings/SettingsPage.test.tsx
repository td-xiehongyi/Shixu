// @vitest-environment jsdom
import { expect, it } from "vitest";
import { SettingsPage } from "./SettingsPage";
import { port, render } from "../../test/d4";
it("model_and_autostart_default_off", async () => {
  const r = await render(<SettingsPage port={port()} />);
  const controls = [
    ...r.host.querySelectorAll<HTMLInputElement>("input[type=checkbox]"),
  ];
  expect(controls.length).toBeGreaterThanOrEqual(3);
  expect(controls.every((c) => !c.checked)).toBe(true);
  expect(r.host.textContent).toContain("尚未接通");
  expect(r.host.textContent).toContain("5 分钟");
  await r.close();
});
import { act } from "react";
import { settings as initialSettings } from "../../test/d4";
import type { SettingsSnapshot, SourceConfig } from "../../contracts/domain";
it("saving_new_source_then_editing_preserves_source_identity", async () => {
  let snapshot: SettingsSnapshot = structuredClone(initialSettings);
  const saved: SourceConfig[] = [];
  const r = await render(
    <SettingsPage
      port={port({
        settingsRead: async () => snapshot,
        saveSourceConfig: async (config) => {
          saved.push(config);
          snapshot = {
            ...snapshot,
            sources: [{ config, epoch: String(saved.length) }],
          };
        },
      })}
    />,
  );
  r.host.querySelector<HTMLInputElement>("input[name=account]")!.value =
    "synthetic";
  await r.click("保存来源");
  await r.click("保存来源");
  expect(saved).toHaveLength(2);
  expect(saved[0].source_id).toBe(saved[1].source_id);
  await r.close();
});
it("shows_runtime_queues_and_storage_failure_without_claiming_connected", async () => {
  const snapshot: SettingsSnapshot = {
    ...initialSettings,
    runtime: {
      running: true,
      pending_rules: 3,
      attachment_queue: 2,
      model_queue: 1,
      last_calendar_commit: null,
      last_error: "STORAGE_FULL",
      sources: [],
    },
  };
  const r = await render(
    <SettingsPage port={port({ settingsRead: async () => snapshot })} />,
  );
  expect(r.host.textContent).toContain("待入历 3");
  expect(r.host.textContent).toContain("附件 2");
  expect(r.host.textContent).toContain("模型 1");
  expect(r.host.textContent).toContain("存储空间不足");
  expect(r.host.textContent).toContain("QQ 未连接");
  await r.close();
});
