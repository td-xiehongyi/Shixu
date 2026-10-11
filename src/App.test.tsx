// @vitest-environment jsdom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { expect, it, vi } from "vitest";
const calls = vi.hoisted(() => [] as unknown[]);
vi.mock("./contracts/bridge", () => ({
  mainBridge: {
    setVaultViewVisible: async (...args: unknown[]) => {
      calls.push(args);
    },
    getVaultLockState: async () => false,
  },
}));
vi.mock("./features/calendar/CalendarPage", () => ({
  CalendarPage: () => null,
}));
vi.mock("./features/notifications/NotificationsPage", () => ({
  NotificationsPage: () => null,
}));
vi.mock("./features/settings/SettingsPage", () => ({
  SettingsPage: () => null,
}));
vi.mock("./features/vault/VaultWindow", () => ({ VaultWindow: () => null }));
import App from "./App";
it("navigation_uses_one_embedded_view_and_latest_revision", async () => {
  (
    globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }
  ).IS_REACT_ACT_ENVIRONMENT = true;
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  try {
    await act(async () => root.render(<App />));
    const click = async (text: string) =>
      act(async () =>
        [...host.querySelectorAll("button")]
          .find((b) => b.textContent === text)!
          .click(),
      );
    await click("密码库");
    await click("通知");
    await click("密码库");
    expect(calls.slice(-3)).toEqual([
      [true, "1"],
      [false, "2"],
      [true, "3"],
    ]);
    expect(host.textContent).not.toContain("独立密码窗口");
  } finally {
    await act(async () => root.unmount());
    host.remove();
  }
});
