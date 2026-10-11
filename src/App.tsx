import { useEffect, useRef, useState } from "react";
import { Bell } from "@phosphor-icons/react/dist/csr/Bell";
import { CalendarBlank } from "@phosphor-icons/react/dist/csr/CalendarBlank";
import { GearSix } from "@phosphor-icons/react/dist/csr/GearSix";
import { House } from "@phosphor-icons/react/dist/csr/House";
import { LockSimple } from "@phosphor-icons/react/dist/csr/LockSimple";
import { isTauri } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
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
  const [vaultState, setVaultState] = useState<
    "unknown" | "locked" | "unlocked"
  >("unknown");
  const navigation = useRef(0n);
  const isVault =
    new URLSearchParams(location.search).get("window") === "vault";
  useEffect(() => {
    if (isVault) return;
    let active = true;
    let remove: (() => void) | undefined;
    let removeFailure: (() => void) | undefined;
    const update = (value: boolean) => {
      if (active) setVaultState(value ? "unlocked" : "locked");
    };
    void (async () => {
      if (isTauri()) {
        const cleanup = await listen<boolean>(
          "vault_lock_state",
          (event) => {
            if (typeof event.payload === "boolean") update(event.payload);
          },
          { target: { kind: "Webview", label: "main" } },
        );
        if (!active) {
          cleanup();
          return;
        }
        remove = cleanup;
        const failureCleanup = await listen<boolean>(
          "vault_view_failed",
          (event) => {
            if (active) {
              setMessage(
                event.payload ? "密码库暂不可用，请在密码区点击重试。" : "",
              );
              if (event.payload) setVaultState("unknown");
            }
          },
          { target: { kind: "Webview", label: "main" } },
        );
        if (!active) {
          failureCleanup();
          return;
        }
        removeFailure = failureCleanup;
      }
      update(await mainBridge.getVaultLockState());
    })().catch(() => {
      if (active) setVaultState("unknown");
    });
    return () => {
      active = false;
      remove?.();
      removeFailure?.();
    };
  }, [isVault]);
  function navigate(next: Module) {
    setModule(next);
    if (next !== "vault" && navigation.current === 0n) return;
    const revision = ++navigation.current;
    setMessage("");
    void mainBridge
      .setVaultViewVisible(next === "vault", String(revision))
      .catch(() => {
        if (navigation.current === revision) {
          setMessage("密码库暂不可用，请重新点击密码库。");
          setVaultState("unknown");
        }
      });
  }
  if (new URLSearchParams(location.search).get("window") === "vault")
    return <VaultWindow />;
  function today() {
    navigate("calendar");
    setTodayKey((v) => v + 1);
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
            onClick={() => navigate("calendar")}
            aria-current={module === "calendar" ? "page" : undefined}
          >
            <CalendarBlank size={25} />
            <span>日历</span>
          </button>
          <button
            className={module === "notifications" ? "active" : ""}
            onClick={() => navigate("notifications")}
            aria-current={module === "notifications" ? "page" : undefined}
          >
            <Bell size={25} />
            <span>通知</span>
          </button>
          <button
            className={module === "vault" ? "active" : ""}
            onClick={() => navigate("vault")}
            aria-current={module === "vault" ? "page" : undefined}
          >
            <LockSimple size={25} />
            <span>密码库</span>
          </button>
          <button
            className={module === "settings" ? "active" : ""}
            onClick={() => navigate("settings")}
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
            <span>
              {vaultState === "unlocked"
                ? "密码库已解锁"
                : vaultState === "locked"
                  ? "密码库已锁定"
                  : "密码库状态暂不可用"}
            </span>
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
        <main className="module-content vault-loading">
          <p role="status">{message || "正在打开密码库…"}</p>
        </main>
      ) : (
        <SettingsPage demo={demo} onDemo={setDemo} />
      )}
    </div>
  );
}
