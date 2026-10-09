// @vitest-environment jsdom
import { expect, it } from "vitest";
import { NotificationsPage } from "./NotificationsPage";
import { notice, port, render } from "../../test/d4";
it("missing_part_is_visible_beside_success", async () => {
  const r = await render(<NotificationsPage port={port()} />);
  expect(r.host.textContent).toContain("正文已入历，另 1 张图片无法下载");
  expect(r.host.textContent).toContain("原件不可用");
  await r.close();
});
it("no_html_execution_in_evidence", async () => {
  const r = await render(<NotificationsPage port={port()} />);
  expect(r.host.textContent).toContain(notice.text);
  expect(r.host.querySelector("img")).toBeNull();
  expect(r.host.querySelector("a")).toBeNull();
  await r.close();
});
it("source_revocation_is_not_event_cancellation", async () => {
  const r = await render(
    <NotificationsPage
      port={port({
        notificationList: async () => [
          { ...notice, revoked: true, processing_state: "source_revoked" },
        ],
      })}
    />,
  );
  expect(r.host.textContent).toContain("来源已撤回");
  expect(r.host.textContent).toContain("不等于日程取消");
  await r.close();
});
import { act } from "react";
it("late_attachment_does_not_revert_newer_notice", async () => {
  let resolve!: (v: import("../../contracts/domain").PartResult[]) => void;
  const pending = new Promise<import("../../contracts/domain").PartResult[]>(
    (r) => (resolve = r),
  );
  let revision = "1";
  const r = await render(
    <NotificationsPage
      port={port({
        notificationList: async () => [
          { ...notice, revision, text: revision === "1" ? "旧通知" : "新通知" },
        ],
        notificationParts: async () => (revision === "1" ? pending : []),
      })}
    />,
  );
  revision = "2";
  await r.click("刷新通知");
  await act(async () =>
    resolve([
      {
        part_id: notice.message_key,
        status: "success",
        blocks: [
          {
            part_id: notice.message_key,
            page_or_sheet: null,
            cell_range_or_bbox: null,
            text: "迟到旧附件",
            method: "ocr",
            engine_version: "synthetic",
            quality_flags: [],
          },
        ],
        reason_code: null,
      },
    ]),
  );
  await r.click("来源原文");
  expect(r.host.textContent).toContain("新通知");
  expect(r.host.textContent).not.toContain("迟到旧附件");
  await r.close();
});
