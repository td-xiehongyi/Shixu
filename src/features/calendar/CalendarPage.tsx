import { useEffect, useState } from "react";
import FullCalendar from "@fullcalendar/react";
import dayGridPlugin from "@fullcalendar/daygrid";
import listPlugin from "@fullcalendar/list";
import { CaretLeft } from "@phosphor-icons/react/dist/csr/CaretLeft";
import { CaretRight } from "@phosphor-icons/react/dist/csr/CaretRight";
import { Plus } from "@phosphor-icons/react/dist/csr/Plus";
import { Check } from "@phosphor-icons/react/dist/csr/Check";
import { mainBridge } from "../../contracts/bridge";
import type { CalendarEvent } from "../../contracts/domain";
import { EventDetails, errorText } from "./EventDetails";
const dayLabels = ["周一", "周二", "周三", "周四", "周五", "周六", "周日"];
const hours = Array.from({ length: 11 }, (_, i) => i + 8);
function monday(date: Date) {
  const d = new Date(date);
  d.setHours(0, 0, 0, 0);
  d.setDate(d.getDate() - ((d.getDay() + 6) % 7));
  return d;
}
function dateKey(d: Date) {
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`;
}
function clock(value: number, timezone: string) {
  return new Intl.DateTimeFormat("en-GB", {
    timeZone: timezone,
    hour: "2-digit",
    minute: "2-digit",
    hourCycle: "h23",
  }).format(new Date(value));
}
function civilTime(value: number, timezone: string) {
  const parts = new Intl.DateTimeFormat("en-CA", {
    timeZone: timezone,
    year: "numeric",
    month: "2-digit",
    day: "2-digit",
  }).formatToParts(new Date(value));
  const get = (t: string) => parts.find((p) => p.type === t)!.value;
  return `${get("year")}-${get("month")}-${get("day")}T${clock(value, timezone)}:00`;
}
export function CalendarPage({
  port = mainBridge,
  demo = false,
  todayKey = 0,
}: {
  port?: typeof mainBridge;
  demo?: boolean;
  todayKey?: number;
}) {
  const [week, setWeek] = useState(() =>
    monday(demo ? new Date(2026, 9, 9) : new Date()),
  );
  const [events, setEvents] = useState<CalendarEvent[]>([]);
  const [message, setMessage] = useState("");
  const [view, setView] = useState("week");
  const [cancelled, setCancelled] = useState(false);
  const [refresh, setRefresh] = useState(0);
  const [selected, setSelected] = useState<CalendarEvent | null | undefined>(
    undefined,
  );
  useEffect(() => {
    setWeek(monday(demo ? new Date(2026, 9, 9) : new Date()));
    setSelected(undefined);
  }, [demo, todayKey]);
  useEffect(() => {
    let active = true;
    setEvents([]);
    if (demo) return;
    const from = new Date(week);
    let through = new Date(week);
    if (view === "week") through.setDate(through.getDate() + 6);
    else {
      from.setDate(1);
      through = new Date(from.getFullYear(), from.getMonth() + 1, 0);
    }
    port
      .calendarQuery({
        from_date: dateKey(from),
        through_date: dateKey(through),
        statuses: cancelled ? ["active", "cancelled"] : ["active"],
        include_pending: true,
      })
      .then((v) => {
        if (active) {
          setEvents(v);
          setMessage("");
        }
      })
      .catch((e) => {
        if (active) setMessage(errorText(e));
      });
    return () => {
      active = false;
    };
  }, [port, demo, week, view, cancelled, refresh]);
  const current = demo ? new Date(2026, 9, 9) : new Date();
  const days = Array.from({ length: 7 }, (_, i) => {
    const d = new Date(week);
    d.setDate(d.getDate() + i);
    return d;
  });
  const range = `${week.getFullYear()}年${week.getMonth() + 1}月${week.getDate()}日—${days[6].getMonth() + 1}月${days[6].getDate()}日`;
  const demoEvents: CalendarEvent[] = [
    {
      event_id: "11111111-1111-4111-8111-111111111111",
      title: "高等数学考试",
      kind: "exam",
      time_precision: "exact",
      local_date: "2026-10-09",
      start_at: Date.parse("2026-10-09T09:00:00+08:00"),
      end_at: Date.parse("2026-10-09T11:00:00+08:00"),
      timezone: "Asia/Shanghai",
      raw_time_text: "模拟时间",
      location: null,
      status: "active",
      revision: "1",
      user_overrides: [],
    },
    {
      event_id: "22222222-2222-4222-8222-222222222222",
      title: "英语模拟考试",
      kind: "exam",
      time_precision: "exact",
      local_date: "2026-10-11",
      start_at: Date.parse("2026-10-11T14:00:00+08:00"),
      end_at: Date.parse("2026-10-11T16:00:00+08:00"),
      timezone: "Asia/Shanghai",
      raw_time_text: "模拟时间",
      location: null,
      status: "active",
      revision: "1",
      user_overrides: [],
    },
  ];
  const visible = (demo ? demoEvents : events).filter(
    (e) => e.status === "active" || (cancelled && e.status === "cancelled"),
  );
  function shift(delta: number) {
    setWeek((w) => {
      const d = new Date(w);
      if (view === "week") d.setDate(d.getDate() + delta);
      else d.setMonth(d.getMonth() + Math.sign(delta), 1);
      return d;
    });
  }
  function today() {
    setWeek(monday(current));
  }
  return (
    <main className="calendar-main">
      <header className="calendar-top">
        <div className="title-row">
          <h1>日历</h1>
          <button
            className="primary"
            disabled={demo}
            title={demo ? "关闭演示数据后可新建真实日程" : undefined}
            onClick={() => setSelected(null)}
          >
            <Plus size={22} /> 新建日程
          </button>
        </div>
        <div className="week-row">
          <div className="week-title">
            {view === "week"
              ? range
              : `${week.getFullYear()}年${week.getMonth() + 1}月`}
            {demo && <span className="demo-badge">演示数据</span>}
          </div>
          <div className="week-controls">
            <button
              onClick={() => shift(-7)}
              aria-label={view === "week" ? "上一周" : "上一月"}
            >
              <CaretLeft size={20} />
            </button>
            <button onClick={today}>今天</button>
            <button
              onClick={() => shift(7)}
              aria-label={view === "week" ? "下一周" : "下一月"}
            >
              <CaretRight size={20} />
            </button>
          </div>
        </div>
      </header>
      {!demo && message && (
        <p className="service-note" role="status">
          {message}
        </p>
      )}
      <div className="calendar-options">
        <label>
          视图
          <select
            aria-label="日历视图"
            value={view}
            onChange={(e) => setView(e.target.value)}
          >
            <option value="week">周</option>
            <option value="month">月</option>
            <option value="agenda">议程</option>
          </select>
        </label>
        <label>
          <input
            type="checkbox"
            checked={cancelled}
            onChange={(e) => setCancelled(e.target.checked)}
          />
          显示已取消
        </label>
        <button onClick={() => setRefresh((v) => v + 1)}>刷新</button>
      </div>
      <p className="automatic-note">
        通知会自动加入日历，无需逐条确认；可在详情中修改或撤销。
      </p>
      {view !== "week" ? (
        <FullCalendar
          key={`${view}-${dateKey(week)}`}
          plugins={[dayGridPlugin, listPlugin]}
          initialView={view === "month" ? "dayGridMonth" : "listMonth"}
          initialDate={dateKey(week)}
          headerToolbar={false}
          locale="zh-cn"
          firstDay={1}
          editable={false}
          selectable={false}
          events={visible
            .filter(
              (e) =>
                e.time_precision !== "unknown_date" &&
                e.time_precision !== "date_only",
            )
            .map((e) => ({
              id: e.event_id,
              title: e.title,
              start:
                e.time_precision === "explicit_all_day"
                  ? e.local_date!
                  : civilTime(e.start_at!, e.timezone),
              end: e.end_at ? civilTime(e.end_at, e.timezone) : undefined,
              allDay: e.time_precision === "explicit_all_day",
            }))}
          eventClick={(info) =>
            !demo &&
            setSelected(visible.find((e) => e.event_id === info.event.id)!)
          }
        />
      ) : (
        <div className="calendar-scroll">
          <div className="calendar-grid">
            <div className="time-gutter">
              {hours.map((h) => (
                <span
                  key={h}
                  style={{
                    top: `calc(var(--header-height) + ${h - 8} * var(--hour-height) - 12px)`,
                  }}
                >
                  {String(h).padStart(2, "0")}:00
                </span>
              ))}
            </div>
            <div className="week-grid">
              <div className="day-headers">
                {days.map((d, i) => (
                  <div
                    key={dateKey(d)}
                    className={
                      dateKey(d) === dateKey(current) ? "current-day" : ""
                    }
                  >
                    <span>{dayLabels[i]}</span>
                    <strong>
                      {dateKey(d) === dateKey(current)
                        ? d.getDate()
                        : `${d.getMonth() + 1}月${d.getDate()}日`}
                    </strong>
                  </div>
                ))}
              </div>
              <div className="hour-grid">
                <div className="day-columns">
                  {days.map((d) => (
                    <div
                      key={dateKey(d)}
                      className={
                        dateKey(d) === dateKey(current) ? "current-day" : ""
                      }
                    />
                  ))}
                </div>
                {hours.map((h) => (
                  <div
                    className="hour-line"
                    key={h}
                    style={{ top: `calc(${h - 8} * var(--hour-height))` }}
                  />
                ))}
                {demo && dateKey(week) === "2026-10-05" && (
                  <>
                    <article
                      className="event exam"
                      style={{
                        left: "calc(4 * 100% / 7 + 3px)",
                        top: "calc(var(--hour-height) * 1)",
                        height: "calc(var(--hour-height) * 2)",
                      }}
                    >
                      <span>09:00–11:00</span>
                      <strong>高等数学考试</strong>
                      <small>示例班级群</small>
                    </article>
                    <article
                      className="event manual"
                      style={{
                        left: "calc(2 * 100% / 7 + 3px)",
                        top: "calc(var(--hour-height) * 6)",
                        height: 54,
                      }}
                    >
                      <span>14:00</span>
                      <strong>
                        <Check size={16} />
                        资料整理
                      </strong>
                    </article>
                    <article
                      className="event english"
                      style={{
                        left: "calc(6 * 100% / 7 + 3px)",
                        top: "calc(var(--hour-height) * 6)",
                        height: "calc(var(--hour-height) * 2)",
                      }}
                    >
                      <span>14:00–16:00</span>
                      <strong>英语模拟考试</strong>
                    </article>
                  </>
                )}
                {!demo &&
                  visible.map((event) => {
                    const i = days.findIndex(
                      (d) => dateKey(d) === event.local_date,
                    );
                    if (
                      i < 0 ||
                      event.time_precision !== "exact" ||
                      event.start_at === null
                    )
                      return null;
                    const [h, m] = clock(event.start_at, event.timezone)
                      .split(":")
                      .map(Number);
                    const hour = h + m / 60;
                    if (hour < 8 || hour > 18) return null;
                    return (
                      <article
                        className="event exam"
                        key={event.event_id}
                        onClick={() => setSelected(event)}
                        role="button"
                        tabIndex={0}
                        onKeyDown={(e) => {
                          if (e.key === "Enter") setSelected(event);
                        }}
                        style={{
                          left: `calc(${i} * 100% / 7 + 3px)`,
                          top: `calc(var(--hour-height) * ${hour - 8})`,
                          height: 70,
                        }}
                      >
                        <span>{clock(event.start_at, event.timezone)}</span>
                        <strong>{event.title}</strong>
                      </article>
                    );
                  })}
              </div>
            </div>
          </div>
        </div>
      )}
      <section className="calendar-list" aria-label="已知日期">
        <h3>已知日期 · 时间待定 / 全天</h3>
        {visible
          .filter(
            (e) =>
              e.time_precision === "date_only" ||
              e.time_precision === "explicit_all_day",
          )
          .map((e) => (
            <button
              key={e.event_id}
              disabled={demo}
              onClick={() => setSelected(e)}
            >
              {e.local_date} · {e.title} ·{" "}
              {e.time_precision === "date_only" ? "时间待定" : "明确全天"}{" "}
              {e.status === "cancelled" ? "（已取消）" : ""}
            </button>
          ))}
      </section>
      <section className="calendar-list" aria-label="日期待定">
        <h3>日期待定</h3>
        {visible
          .filter((e) => e.time_precision === "unknown_date")
          .map((e) => (
            <button
              key={e.event_id}
              disabled={demo}
              onClick={() => setSelected(e)}
            >
              {e.title} · 日期待定
            </button>
          ))}
      </section>
      <section className="calendar-list" aria-label="本期全部日程">
        <h3>本期日程</h3>
        {visible
          .filter((e) => e.time_precision === "exact")
          .map((e) => (
            <button
              key={e.event_id}
              disabled={demo}
              onClick={() => setSelected(e)}
            >
              {e.local_date} · {clock(e.start_at!, e.timezone)} · {e.title}{" "}
              {e.status === "cancelled" ? "（已取消）" : ""}
            </button>
          ))}
      </section>
      {selected !== undefined && (
        <EventDetails
          key={selected?.event_id ?? "new"}
          event={selected}
          port={port}
          onClose={() => setSelected(undefined)}
          onChanged={() => setRefresh((v) => v + 1)}
        />
      )}
    </main>
  );
}
