import { useState } from "react";
import { Bell } from "@phosphor-icons/react/dist/csr/Bell";
import { CalendarBlank } from "@phosphor-icons/react/dist/csr/CalendarBlank";
import { GearSix } from "@phosphor-icons/react/dist/csr/GearSix";
import { House } from "@phosphor-icons/react/dist/csr/House";
import { LockSimple } from "@phosphor-icons/react/dist/csr/LockSimple";
import { mainBridge } from "./contracts/bridge";
import { VaultWindow } from "./features/vault/VaultWindow";

import { CalendarPage } from "./features/calendar/CalendarPage";
import { NotificationsPage } from "./features/notifications/NotificationsPage";
import { SettingsPage } from "./features/settings/SettingsPage";
type Module = "calendar" | "notifications" | "vault" | "settings";
export default function App() {
  const [demo, setDemo] = useState(
    () => new URLSearchParams(location.search).get("demo") === "1",
  );
  const [module, setModule] = useState<Module>("calendar");
  const [message, setMessage] = useState("");
  const [todayKey, setTodayKey] = useState(0);
  if (new URLSearchParams(location.search).get("window") === "vault")
    return <VaultWindow />;
  function today() {
    setModule("calendar");
    setTodayKey((v) => v + 1);
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
          <CalendarPage demo={demo} todayKey={todayKey} />
          <NotificationsPage demo={demo} />
        </>
      ) : module === "notifications" ? (
        <main className="module-content">
          <NotificationsPage demo={demo} full />
        </main>
      ) : module === "vault" ? (
        <main className="module-content">
          <h1>密码库</h1>
          <p>主密码只锁密码库，日历与通知独立运行。</p>
          <button className="primary" onClick={vault}>
            打开独立密码窗口
          </button>
          <p role="status">{message}</p>
        </main>
      ) : (
        <SettingsPage demo={demo} onDemo={setDemo} />
      )}
    </div>
  );
}
