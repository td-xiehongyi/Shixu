// @vitest-environment jsdom
import { expect, it, vi } from "vitest";
import { CalendarPage } from "./CalendarPage";
import { event, port, render } from "../../test/d4";
it("unknown_date_has_no_calendar_cell", async () => {
  const r = await render(
    <CalendarPage
      port={port({
        calendarQuery: async () => [
          { ...event, time_precision: "unknown_date", local_date: null },
        ],
      })}
    />,
  );
  expect(
    r.host.querySelector('[aria-label="日期待定"]')?.textContent,
  ).toContain(event.title);
  expect(r.host.querySelector(".hour-grid")?.textContent).not.toContain(
    event.title,
  );
  await r.close();
});
it("date_only_is_not_all_day", async () => {
  const r = await render(<CalendarPage port={port()} />);
  expect(r.host.textContent).toContain("时间待定");
  expect(
    r.host.querySelector('[aria-label="已知日期"]')?.textContent,
  ).toContain(event.title);
  await r.close();
});
it("automatic_insert_needs_no_confirmation", async () => {
  const confirm = vi.spyOn(window, "confirm");
  const r = await render(<CalendarPage port={port()} />);
  expect(r.host.textContent).toContain("自动加入日历");
  expect(confirm).not.toHaveBeenCalled();
  await r.close();
  confirm.mockRestore();
});
it("history_undo_and_user_override", async () => {
  const undo = vi.fn(async () => ({
    ...event,
    status: "removed" as const,
    revision: "2",
  }));
  const r = await render(<CalendarPage port={port({ calendarUndo: undo })} />);
  await r.click(event.title);
  expect(r.host.textContent).toContain("修改历史");
  await r.click("撤销此变更");
  expect(undo).toHaveBeenCalledWith({
    change_id: event.event_id,
    expected_revision: "1",
  });
  await r.close();
});
import { act } from "react";
import { EventDetails } from "./EventDetails";
it("title_edit_preserves_full_time_provenance_and_other_automatic_fields", async () => {
  const edit = vi.fn(async () => event);
  const r = await render(
    <EventDetails
      event={event}
      port={port({ calendarEdit: edit })}
      onClose={() => {}}
      onChanged={() => {}}
    />,
  );
  const title = r.host.querySelector<HTMLInputElement>("input[name=title]")!;
  title.value = "人工标题";
  await act(async () =>
    r.host
      .querySelector("form")!
      .dispatchEvent(new Event("submit", { bubbles: true, cancelable: true })),
  );
  expect(edit).toHaveBeenCalledWith(
    event.event_id,
    "1",
    expect.objectContaining({
      title: "人工标题",
      time: null,
      location: null,
      status: null,
    }),
  );
  await r.close();
});
it("cancelled_filter_is_explicit_and_manual_creation_requires_persistence", async () => {
  const seen: boolean[] = [];
  const create = vi.fn(async () => event);
  const r = await render(
    <CalendarPage
      port={port({
        calendarQuery: async (q) => {
          seen.push(q.statuses.includes("cancelled"));
          return [{ ...event, status: "cancelled" }];
        },
        createManualEvent: create,
      })}
    />,
  );
  expect(r.host.textContent).not.toContain(event.title);
  await act(async () =>
    r.host.querySelector<HTMLInputElement>("input[type=checkbox]")!.click(),
  );
  expect(r.host.textContent).toContain(event.title);
  expect(seen).toContain(true);
  await r.click("新建日程");
  r.host.querySelector<HTMLInputElement>("input[name=title]")!.value =
    "手动测试";
  r.host.querySelector<HTMLInputElement>("input[name=date]")!.value =
    "2026-10-09";
  await act(async () =>
    r.host
      .querySelector("form")!
      .dispatchEvent(new Event("submit", { bubbles: true, cancelable: true })),
  );
  expect(create).toHaveBeenCalledWith(
    expect.objectContaining({
      title: "手动测试",
      expected_revision: "0",
      time: expect.objectContaining({ precision: "date_only", start_at: null }),
    }),
  );
  await r.close();
});
it("switching_views_uses_month_and_agenda_without_dragging", async () => {
  const r = await render(<CalendarPage port={port()} />);
  const select = r.host.querySelector<HTMLSelectElement>(
    'select[aria-label="日历视图"]',
  )!;
  await act(async () => {
    select.value = "month";
    select.dispatchEvent(new Event("change", { bubbles: true }));
  });
  expect(r.host.querySelector(".fc-dayGridMonth-view")).not.toBeNull();
  await act(async () => {
    select.value = "agenda";
    select.dispatchEvent(new Event("change", { bubbles: true }));
  });
  expect(r.host.querySelector(".fc-listMonth-view")).not.toBeNull();
  expect(r.host.querySelector(".fc-event-draggable")).toBeNull();
  await r.close();
});
