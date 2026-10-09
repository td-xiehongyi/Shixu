import { useEffect, useState } from "react";
import { Bell } from "@phosphor-icons/react/dist/csr/Bell";
import { CalendarBlank } from "@phosphor-icons/react/dist/csr/CalendarBlank";
import { CaretLeft } from "@phosphor-icons/react/dist/csr/CaretLeft";
import { CaretRight } from "@phosphor-icons/react/dist/csr/CaretRight";
import { Check } from "@phosphor-icons/react/dist/csr/Check";
import { GearSix } from "@phosphor-icons/react/dist/csr/GearSix";
import { House } from "@phosphor-icons/react/dist/csr/House";
import { LockSimple } from "@phosphor-icons/react/dist/csr/LockSimple";
import { Plus } from "@phosphor-icons/react/dist/csr/Plus";
import { Sparkle } from "@phosphor-icons/react/dist/csr/Sparkle";
import { UsersThree } from "@phosphor-icons/react/dist/csr/UsersThree";
import { mainBridge } from "./contracts/bridge";
import type { CalendarEvent } from "./contracts/domain";

type Module = "calendar" | "notifications" | "vault" | "settings";
const dayLabels = ["周一", "周二", "周三", "周四", "周五", "周六", "周日"];
const hours = Array.from({ length: 11 }, (_, i) => i + 8);
const notices = [
  {
    group: "示例班级群",
    stamp: "今天 08:10",
    text: "高数考试09:00，教学楼A201",
    state: "已自动加入日历",
  },
  {
    group: "示例英语群",
    stamp: "昨天 18:30",
    text: "周日14:00英语模拟考试",
    state: "已自动加入日历",
  },
  {
    group: "示例活动群",
    stamp: "昨天 16:20",
    text: "交流会时间另行通知",
    state: "时间待定",
  },
];
function monday(date: Date): Date {
  const d = new Date(date);
  d.setHours(0, 0, 0, 0);
  d.setDate(d.getDate() - ((d.getDay() + 6) % 7));
  return d;
}
function dateKey(d: Date): string {
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`;
}
function NotificationPanel({
  demo,
  full = false,
}: {
  demo: boolean;
  full?: boolean;
}) {
  return (
    <aside
      className={`notifications ${full ? "full" : ""}`}
      aria-label="QQ通知"
    >
      <h2>QQ通知</h2>
      <p className={`collect-state ${demo ? "demo-state" : ""}`}>
        <span className="status-dot" />
        {demo ? "已选群正在采集" : "QQ 未连接"}
      </p>
      <p className="run-hint">
        {demo ? "仅电脑开机时运行" : "尚未配置通知来源"}
      </p>
      {demo ? (
        <div className="notice-list">
          {notices.map((n) => (
            <article className="notice" key={n.group}>
              <div className="group-icon">
                <UsersThree size={23} weight="fill" />
              </div>
              <div className="notice-content">
                <div className="notice-heading">
                  <h3>{n.group}</h3>
                  <button
                    disabled
                    className="source-link"
                    title="来源详情将在通知功能中开放"
                  >
                    来源原文 <CaretRight size={14} />
                  </button>
                </div>
                <div className="notice-meta">
                  <span>{n.stamp}</span>
                </div>
                <p>{n.text}</p>
                <span
                  className={`notice-state ${n.state === "时间待定" ? "pending" : ""}`}
                >
                  {n.state}
                </span>
              </div>
            </article>
          ))}
        </div>
      ) : (
        <div className="empty-notice">
          <Bell size={28} />
          <p>连接 QQ 来源后，通知会显示在这里。</p>
        </div>
      )}
      <div className="future-agent" aria-disabled="true">
        <Sparkle size={23} />
        <span>个人智能体</span>
        <small>后续开放</small>
        <CaretRight size={16} />
      </div>
    </aside>
  );
}
function VaultView() {
  return (
    <main className="vault-window">
      <LockSimple size={40} />
      <h1>密码库已锁定</h1>
      <p>密码库将在独立窗口中运行。</p>
      <p className="muted">密码引擎尚未验证，暂不可解锁或保存条目。</p>
      <span className="availability">暂不可用</span>
    </main>
  );
}
export default function App() {
  // Demo opt-in is presentation only; never swaps or intercepts the actual native bridge.
  const [demo, setDemo] = useState(
    () => new URLSearchParams(location.search).get("demo") === "1",
  );
  const [module, setModule] = useState<Module>("calendar");
  const [week, setWeek] = useState(() =>
    monday(demo ? new Date(2026, 9, 9) : new Date()),
  );
  const [events, setEvents] = useState<CalendarEvent[]>([]);
  const [message, setMessage] = useState("");
  useEffect(() => {
    if (demo) return;
    let active = true;
    const through = new Date(week);
    through.setDate(through.getDate() + 6);
    mainBridge
      .calendarQuery({
        from_date: dateKey(week),
        through_date: dateKey(through),
        statuses: ["active", "cancelled"],
        include_pending: false,
      })
      .then((result) => {
        if (active) {
          setEvents(result);
          setMessage("");
        }
      })
      .catch(() => {
        if (active) {
          setEvents([]);
          setMessage("日历服务暂不可用");
        }
      });
    return () => {
      active = false;
    };
  }, [demo, week]);
  if (new URLSearchParams(location.search).get("window") === "vault")
    return <VaultView />;
  const days = Array.from({ length: 7 }, (_, i) => {
    const d = new Date(week);
    d.setDate(d.getDate() + i);
    return d;
  });
  const current = demo ? new Date(2026, 9, 9) : new Date();
  const range = `${week.getFullYear()}年${week.getMonth() + 1}月${week.getDate()}日—${days[6].getMonth() + 1}月${days[6].getDate()}日`;
  function today() {
    setModule("calendar");
    setWeek(monday(current));
  }
  function shift(delta: number) {
    setWeek((w) => {
      const next = new Date(w);
      next.setDate(next.getDate() + delta);
      return next;
    });
  }
  async function vault() {
    setModule("vault");
    try {
      await mainBridge.showVaultWindow();
      setMessage("密码库已在独立窗口打开");
    } catch {
      setMessage("独立密码窗口需要 Windows 桌面应用");
    }
  }
  return (
    <div className="app-shell">
      <nav className="sidebar" aria-label="主导航">
        <div className="brand">
          <strong>拾序</strong>
          <span>SHIXU</span>
        </div>
        <div className="navigation">
          <button onClick={today}>
            <House size={25} />
            <span>今天</span>
          </button>
          <button
            className={module === "calendar" ? "active" : ""}
            onClick={() => setModule("calendar")}
            aria-current={module === "calendar" ? "page" : undefined}
          >
            <CalendarBlank size={25} />
            <span>日历</span>
          </button>
          <button
            className={module === "notifications" ? "active" : ""}
            onClick={() => setModule("notifications")}
            aria-current={module === "notifications" ? "page" : undefined}
          >
            <Bell size={25} />
            <span>通知</span>
          </button>
          <button
            className={module === "vault" ? "active" : ""}
            onClick={vault}
            aria-current={module === "vault" ? "page" : undefined}
          >
            <LockSimple size={25} />
            <span>密码库</span>
          </button>
          <button
            className={module === "settings" ? "active" : ""}
            onClick={() => setModule("settings")}
            aria-current={module === "settings" ? "page" : undefined}
          >
            <GearSix size={25} />
            <span>设置</span>
          </button>
        </div>
        <footer className="sidebar-footer">
          {demo && module !== "calendar" && (
            <p>
              <span className="demo-badge">演示数据</span>
            </p>
          )}
          <p>
            <LockSimple size={19} />
            <span>密码库已锁定</span>
          </p>
          <small>{demo ? "日历与通知正常运行" : "日历与通知独立运行"}</small>
          {!demo && <small className="actual-state">QQ 未连接</small>}
        </footer>
      </nav>
      {module === "calendar" ? (
        <>
          <main className="calendar-main">
            <header className="calendar-top">
              <div className="title-row">
                <h1>日历</h1>
                <button
                  className="primary"
                  disabled
                  title="手动日程将在日历功能中开放"
                >
                  <Plus size={22} /> 新建日程
                </button>
              </div>
              <div className="week-row">
                <div className="week-title">
                  {range}
                  {demo && <span className="demo-badge">演示数据</span>}
                </div>
                <div className="week-controls">
                  <button onClick={() => shift(-7)} aria-label="上一周">
                    <CaretLeft size={20} />
                  </button>
                  <button onClick={today}>今天</button>
                  <button onClick={() => shift(7)} aria-label="下一周">
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
                      events.map((event) => {
                        const i = days.findIndex(
                          (d) => dateKey(d) === event.local_date,
                        );
                        if (i < 0 || event.start_at === null) return null;
                        const start = new Date(event.start_at);
                        const hour = start.getHours() + start.getMinutes() / 60;
                        if (hour < 8 || hour > 18) return null;
                        return (
                          <article
                            className="event exam"
                            key={event.event_id}
                            style={{
                              left: `calc(${i} * 100% / 7 + 3px)`,
                              top: `calc(var(--hour-height) * ${hour - 8})`,
                              height: 70,
                            }}
                          >
                            <span>{event.raw_time_text}</span>
                            <strong>{event.title}</strong>
                          </article>
                        );
                      })}
                  </div>
                </div>
              </div>
            </div>
          </main>
          <NotificationPanel demo={demo} />
        </>
      ) : module === "notifications" ? (
        <main className="module-content">
          <NotificationPanel demo={demo} full />
        </main>
      ) : module === "vault" ? (
        <main className="module-content">
          <LockSimple size={40} />
          <h1>密码库</h1>
          <p>主密码只锁密码库，日历与通知独立运行。</p>
          <button className="primary" onClick={vault}>
            打开独立密码窗口
          </button>
          <p role="status">{message}</p>
          <p className="muted">密码引擎尚未验证，暂不可解锁或保存。</p>
        </main>
      ) : (
        <main className="module-content">
          <h1>设置</h1>
          <p>QQ 来源：未配置</p>
          <p>云端模型：已关闭</p>
          <div className="demo-setting">
            <label>
              <input
                type="checkbox"
                checked={demo}
                onChange={(e) => {
                  const on = e.target.checked;
                  setDemo(on);
                  setWeek(monday(on ? new Date(2026, 9, 9) : new Date()));
                  setMessage("");
                }}
              />
              显示演示数据
            </label>
            <p className="muted">示例群与日程均为虚构，不代表真实连接状态。</p>
          </div>
        </main>
      )}
    </div>
  );
}
