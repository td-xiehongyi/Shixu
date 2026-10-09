// @vitest-environment jsdom
import { expect, it, vi } from "vitest";
import { BackupPanel } from "./BackupPanel";
import { port, render } from "../../test/d4";
it("shows_protection_limits_and_no_export_without_confirmation", async () => {
  const r = await render(<BackupPanel port={port()} />);
  expect(r.host.textContent).toContain("聊天摘录");
  expect(r.host.textContent).toContain("原 Windows 用户");
  expect(r.host.textContent).toContain("旧主密码");
  await r.close();
});
it("does_not_claim_success_when_snapshot_is_unsupported", async () => {
  const r = await render(<BackupPanel port={port()} />);
  await r.click("创建今日日历快照");
  expect(r.host.textContent).not.toContain("快照已保存");
  expect(r.host.textContent).toContain("尚未");
  await r.close();
});
import { act } from "react";
import type { BackupSummary } from "../../contracts/domain";
const summary: BackupSummary = {
  created_at: 0,
  events: "2",
  messages: "3",
  present: "1",
  never_fetched: "1",
  cleaned: "0",
  not_migrated: "1",
};
it("snapshot_success_waits_for_persist_and_reload", async () => {
  let done!: () => void;
  const save = vi.fn(
    () =>
      new Promise<BackupSummary>((resolve) => {
        done = () => resolve(summary);
      }),
  );
  const r = await render(
    <BackupPanel
      port={port({ backupList: async () => [], backupSnapshot: save })}
    />,
  );
  await r.click("创建今日日历快照");
  expect(save).toHaveBeenCalledOnce();
  expect(r.host.textContent).not.toContain("快照已保存");
  await act(async () => done());
  expect(r.host.textContent).toContain("快照已保存");
  await r.close();
});
it("preview_is_not_restore_and_cancel_exports_zero", async () => {
  const restore = vi.fn(async () => {}),
    exporter = vi.fn(async () => "{}");
  const r = await render(
    <BackupPanel
      port={port({
        backupList: async () => [summary],
        backupPreview: async () => ({
          ...summary,
          preview_id: "11111111-1111-4111-8111-111111111111",
        }),
        backupRestore: restore,
        backupExport: exporter,
      })}
    />,
  );
  await r.click("预览恢复");
  expect(restore).not.toHaveBeenCalled();
  expect(r.host.textContent).toContain("从未取得 1");
  await r.click("取消恢复");
  expect(restore).not.toHaveBeenCalled();
  await r.click("导出日历 JSON");
  expect(exporter).not.toHaveBeenCalled();
  await r.click("取消导出");
  expect(exporter).not.toHaveBeenCalled();
  await r.click("预览恢复");
  await r.click("确认覆盖恢复");
  expect(restore).toHaveBeenCalledWith(
    "11111111-1111-4111-8111-111111111111",
    true,
  );
  expect(r.host.textContent).toContain("恢复已完成");
  await r.close();
});
