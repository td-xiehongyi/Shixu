// @vitest-environment jsdom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { VaultWindow, type VaultPort } from "./VaultWindow";
import { EntryForm } from "./EntryForm";
import { BridgeError, createVaultBridge } from "../../contracts/bridge";
import type { VaultMutation, VaultSummary } from "../../contracts/domain";
const rows: VaultSummary[] = [1, 2].map((n) => ({
  entry_id: `11111111-1111-4111-8111-11111111111${n}`,
  channel: "测试渠道",
  account: `虚构账号${n}`,
  revision: "9007199254740993",
  created_at: 0,
  updated_at: 0,
}));
let host: HTMLDivElement, root: Root;
beforeEach(() => {
  (
    globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }
  ).IS_REACT_ACT_ENVIRONMENT = true;
  vi.useFakeTimers();
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});
afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
  vi.useRealTimers();
});
function port(extra: Partial<VaultPort> = {}): VaultPort {
  return {
    vaultUnlock: async () => {},
    vaultCreate: async () => {},
    vaultChangeMaster: async () => {},
    vaultList: async () => rows,
    vaultApply: async () => rows[0],
    vaultReveal: async () => new TextEncoder().encode("synthetic-reveal"),
    vaultCopy: async () => {},
    vaultLock: async () => {},
    ...extra,
  } as VaultPort;
}
async function render(p: VaultPort) {
  await act(async () => root.render(<VaultWindow port={p} />));
}
function button(text: string) {
  const b = [...host.querySelectorAll("button")].find(
    (e) => e.textContent === text,
  );
  expect(b, `button ${text}`).toBeTruthy();
  return b!;
}
async function click(text: string) {
  await act(async () => button(text).click());
}
async function fill(label: string, value: string) {
  const input = [...host.querySelectorAll("input")].find(
    (e) => e.getAttribute("aria-label") === label,
  );
  expect(input, `field ${label}`).toBeTruthy();
  await act(async () => {
    Object.getOwnPropertyDescriptor(
      HTMLInputElement.prototype,
      "value",
    )!.set!.call(input, value);
    input!.dispatchEvent(new Event("input", { bubbles: true }));
    input!.dispatchEvent(new Event("change", { bubbles: true }));
  });
}
async function unlock() {
  await fill("主密码", "synthetic-master");
  await click("解锁密码库");
}
function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((r) => (resolve = r));
  return { promise, resolve };
}
it("only_three_business_fields", async () => {
  await act(async () => root.render(<EntryForm onSave={async () => {}} />));
  expect(
    [...host.querySelectorAll("label")].map((l) => l.firstChild?.textContent),
  ).toEqual(["渠道", "账号", "密码"]);
});
it("locked_hides_accounts", async () => {
  await render(port());
  expect(host.textContent).not.toContain("虚构账号");
  await unlock();
  expect(host.textContent).toContain("虚构账号1");
  await click("锁定密码库");
  expect(host.textContent).not.toContain("虚构账号");
});
it("duplicate_account_does_not_overwrite_and_revision_is_exact", async () => {
  let sent: VaultMutation | undefined;
  await render(
    port({
      vaultApply: async (m) => {
        sent = m;
        return rows[1];
      },
    }),
  );
  await unlock();
  expect(host.querySelectorAll("[data-entry-id]").length).toBe(2);
  await act(async () =>
    host.querySelectorAll<HTMLButtonElement>("button[data-edit]")[1].click(),
  );
  await fill("密码", "  synthetic-password  ");
  await click("保存条目");
  expect(sent?.operation).toBe("update");
  if (sent?.operation === "update") {
    expect(sent.id).toBe(rows[1].entry_id);
    expect(sent.expected_revision).toBe(rows[1].revision);
    expect(sent.password.every((b) => b === 0)).toBe(true);
  }
});
it("password_bytes_preserved_at_port_and_wiped_after_settlement", async () => {
  let seen = "";
  let owned: Uint8Array | undefined;
  await act(async () =>
    root.render(
      <EntryForm
        onSave={async (v) => {
          seen = new TextDecoder().decode(v.password);
          owned = v.password;
        }}
      />,
    ),
  );
  await fill("渠道", "测试");
  await fill("账号", "虚构");
  await fill("密码", "  synthetic-password  ");
  await click("保存条目");
  expect(seen).toBe("  synthetic-password  ");
  expect(owned?.every((b) => b === 0)).toBe(true);
  expect(
    host.querySelector<HTMLInputElement>('input[aria-label="密码"]')?.value,
  ).toBe("");
});
it("reveal_masks_at_fifteen_seconds", async () => {
  await render(port());
  await unlock();
  await click("显示密码");
  expect(host.textContent).toContain("synthetic-reveal");
  await act(async () => vi.advanceTimersByTime(15000));
  expect(host.textContent).not.toContain("synthetic-reveal");
});
it("late_reply_after_lock_is_discarded_and_wiped", async () => {
  const d = deferred<Uint8Array>();
  await render(port({ vaultReveal: () => d.promise }));
  await unlock();
  await click("显示密码");
  await click("锁定密码库");
  const secret = new TextEncoder().encode("synthetic-late");
  await act(async () => d.resolve(secret));
  expect(host.textContent).not.toContain("synthetic-late");
  expect(secret.every((b) => b === 0)).toBe(true);
});
it("late_list_after_lock_and_new_unlock_is_discarded", async () => {
  const d = deferred<VaultSummary[]>();
  let n = 0;
  await render(
    port({ vaultList: () => (++n === 1 ? d.promise : Promise.resolve([])) }),
  );
  await unlock();
  await click("锁定密码库");
  await unlock();
  await act(async () => d.resolve(rows));
  expect(host.textContent).not.toContain("虚构账号");
});
it("inactivity_locks_at_five_minutes_without_background_refresh", async () => {
  await render(port());
  await unlock();
  await act(async () => vi.advanceTimersByTime(299999));
  expect(host.textContent).toContain("虚构账号");
  await act(async () => vi.advanceTimersByTime(1));
  expect(host.textContent).not.toContain("虚构账号");
  expect(host.textContent).toContain("密码库已锁定");
});
it("failed_unlock_stays_locked_with_fixed_error", async () => {
  await render(
    port({
      vaultUnlock: async () => {
        throw new BridgeError("AUTH_FAILED");
      },
    }),
  );
  await unlock();
  expect(host.textContent).toContain("主密码错误");
  expect(host.textContent).not.toContain("虚构账号");
});
it("create_requires_two_equal_passwords_and_forget_is_honest", async () => {
  let n = 0;
  await render(
    port({
      vaultCreate: async () => {
        n++;
      },
    }),
  );
  await click("新建密码库");
  await fill("主密码", "synthetic-one");
  await fill("再次输入主密码", "synthetic-two");
  await click("创建密码库");
  expect(n).toBe(0);
  expect(host.textContent).toContain("两次主密码不一致");
  await click("返回解锁");
  await click("忘记主密码");
  expect(host.textContent).toContain("无法找回");
});
it("save_only_reports_success_after_persistence_and_delete_requires_confirmation", async () => {
  const d = deferred<VaultSummary>();
  let n = 0;
  await render(
    port({
      vaultApply: () => {
        n++;
        return d.promise;
      },
    }),
  );
  await unlock();
  await click("新增条目");
  await fill("渠道", "测试");
  await fill("账号", "虚构");
  await fill("密码", "synthetic");
  await click("保存条目");
  expect(host.textContent).not.toContain("条目已保存");
  await act(async () => d.resolve(rows[0]));
  expect(host.textContent).toContain("条目已保存");
  await click("删除");
  expect(n).toBe(1);
  expect(host.textContent).toContain("确认删除");
});
it("search_matches_only_channel_and_account", async () => {
  await render(port());
  await unlock();
  await fill("搜索渠道或账号", "synthetic-reveal");
  expect(host.querySelectorAll("[data-entry-id]").length).toBe(0);
  await fill("搜索渠道或账号", "虚构账号2");
  expect(host.querySelectorAll("[data-entry-id]").length).toBe(1);
});
it("late_reveal_after_mutation_is_discarded", async () => {
  const d = deferred<Uint8Array>();
  await render(port({ vaultReveal: () => d.promise }));
  await unlock();
  await click("显示密码");
  await click("新增条目");
  await fill("渠道", "测试");
  await fill("账号", "虚构");
  await fill("密码", "synthetic");
  await click("保存条目");
  const secret = new TextEncoder().encode("synthetic-stale");
  await act(async () => d.resolve(secret));
  expect(host.textContent).not.toContain("synthetic-stale");
  expect(secret.every((b) => b === 0)).toBe(true);
});
it("late_reveal_after_unmount_is_wiped", async () => {
  const d = deferred<Uint8Array>();
  await render(port({ vaultReveal: () => d.promise }));
  await unlock();
  await click("显示密码");
  await act(async () => root.render(<div>已关闭</div>));
  const secret = new TextEncoder().encode("synthetic-stale");
  await act(async () => d.resolve(secret));
  expect(host.textContent).toBe("已关闭");
  expect(secret.every((b) => b === 0)).toBe(true);
});
it("late_unlock_after_lock_does_not_list_accounts", async () => {
  const d = deferred<void>();
  let listed = 0;
  await render(
    port({
      vaultUnlock: () => d.promise,
      vaultList: async () => {
        listed++;
        return rows;
      },
    }),
  );
  await unlock();
  await click("锁定密码库");
  await act(async () => d.resolve());
  expect(listed).toBe(0);
  expect(host.textContent).not.toContain("虚构账号");
});
it("pagehide_wipes_reveal_and_accounts_immediately", async () => {
  const bytes = new TextEncoder().encode("synthetic-visible");
  await render(port({ vaultReveal: async () => bytes }));
  await unlock();
  await click("显示密码");
  await act(async () => window.dispatchEvent(new Event("pagehide")));
  expect(host.textContent).not.toContain("虚构账号");
  expect(bytes.every((b) => b === 0)).toBe(true);
});
it("change_master_waits_for_persistence_and_wipes_owned_bytes", async () => {
  let current = "",
    next = "";
  let buffers: Uint8Array[] = [];
  await render(
    port({
      vaultChangeMaster: async (a, b) => {
        current = new TextDecoder().decode(a);
        next = new TextDecoder().decode(b);
        buffers = [a, b];
      },
    }),
  );
  await unlock();
  await click("更改主密码");
  await fill("主密码", "synthetic-current");
  await fill("新主密码", " synthetic-next ");
  await fill("再次输入主密码", " synthetic-next ");
  await click("保存主密码");
  expect(current).toBe("synthetic-current");
  expect(next).toBe(" synthetic-next ");
  expect(buffers.every((a) => a.every((b) => b === 0))).toBe(true);
  expect(host.textContent).toContain("主密码已更改");
});
it("failed_save_never_reports_success_or_raw_error", async () => {
  await render(
    port({
      vaultApply: async () => {
        throw "synthetic-private-provider-error";
      },
    }),
  );
  await unlock();
  await click("新增条目");
  await fill("渠道", "测试");
  await fill("账号", "虚构");
  await fill("密码", "synthetic");
  await click("保存条目");
  expect(host.textContent).not.toContain("条目已保存");
  expect(host.textContent).not.toContain("synthetic-private-provider-error");
  expect(host.textContent).toContain("暂不可用");
});
it("native_locked_copy_immediately_hides_accounts", async () => {
  await render(
    port({
      vaultCopy: async () => {
        throw new BridgeError("LOCKED");
      },
    }),
  );
  await unlock();
  await click("复制密码");
  expect(host.textContent).not.toContain("虚构账号");
  expect(host.textContent).toContain("密码库已锁定");
});
it("unknown_save_failure_fails_closed", async () => {
  await render(
    port({
      vaultApply: async () => {
        throw "synthetic-private-provider-error";
      },
    }),
  );
  await unlock();
  await click("新增条目");
  await fill("渠道", "测试");
  await fill("账号", "虚构");
  await fill("密码", "synthetic");
  await click("保存条目");
  expect(host.textContent).not.toContain("虚构账号");
  expect(host.textContent).toContain("密码库已锁定");
});

// D3-R1: both injected unknown failures and native-normalized failures revoke UI authority.
it.each(["raw", "normalized"] as const)(
  "unknown_copy_%s_failure_revokes_accounts_and_reveal",
  async (kind) => {
    const bytes = new TextEncoder().encode("synthetic-revealed");
    const copyPort = createVaultBridge(async () => {
      throw new Error("synthetic-transport-detail");
    });
    await render(
      port({
        vaultReveal: async () => bytes,
        vaultCopy:
          kind === "raw"
            ? async () => {
                throw new Error("synthetic-transport-detail");
              }
            : copyPort.vaultCopy,
      }),
    );
    await unlock();
    await click("显示密码");
    expect(host.textContent).toContain("synthetic-revealed");
    await click("复制密码");
    expect(host.textContent).not.toContain("synthetic-revealed");
    expect(host.textContent).not.toContain("虚构账号");
    expect(host.textContent).toContain("密码库已锁定");
    expect(host.textContent).toContain("暂不可用");
    expect(host.textContent).not.toContain("synthetic-transport-detail");
    expect(bytes.every((b) => b === 0)).toBe(true);
  },
);
// D3-R3: observation handles retain the real detached DOM inputs solely to verify reset.
it("current_change_master_form_is_reset_on_actual_unmount", async () => {
  await render(port());
  await unlock();
  await click("更改主密码");
  await fill("主密码", "synthetic-current");
  await fill("新主密码", "synthetic-next");
  await fill("再次输入主密码", "synthetic-next");
  const inputs = [
    ...host.querySelectorAll<HTMLInputElement>('input[type="password"]'),
  ];
  expect(inputs.map((x) => x.value)).toEqual([
    "synthetic-current",
    "synthetic-next",
    "synthetic-next",
  ]);
  await act(async () => root.render(<div>已关闭</div>));
  expect(inputs.map((x) => x.value)).toEqual(["", "", ""]);
});
it("manual_lock_resets_current_change_master_form_control", async () => {
  await render(port());
  await unlock();
  await click("更改主密码");
  await fill("主密码", "synthetic-current");
  await fill("新主密码", "synthetic-next");
  await fill("再次输入主密码", "synthetic-next");
  const inputs = [
    ...host.querySelectorAll<HTMLInputElement>('input[type="password"]'),
  ];
  await click("锁定密码库");
  expect(inputs.map((x) => x.value)).toEqual(["", "", ""]);
});
it("cancel_detaches_and_resets_current_change_master_form", async () => {
  await render(port());
  await unlock();
  await click("更改主密码");
  await fill("主密码", "synthetic-current");
  await fill("新主密码", "synthetic-next");
  await fill("再次输入主密码", "synthetic-next");
  const inputs = [
    ...host.querySelectorAll<HTMLInputElement>('input[type="password"]'),
  ];
  await click("取消");
  expect(inputs.map((x) => x.value)).toEqual(["", "", ""]);
});

it.each(["account", "password"] as const)(
  "copy_%s_warns_content_persists_without_automatic_cleanup",
  async (field) => {
    const requests: Array<[string, "account" | "password"]> = [];
    let clipboard = "";
    await render(
      port({
        vaultCopy: async (id, copiedField) => {
          requests.push([id, copiedField]);
          clipboard = "synthetic-clipboard";
        },
      }),
    );
    await unlock();
    await click(field === "password" ? "复制密码" : "复制账号");
    expect(requests).toEqual([[rows[0].entry_id, field]]);
    expect(host.textContent).toContain("不会自动清除");
    expect(host.textContent).toContain("覆盖或手动清除");
    expect(host.textContent).not.toContain("synthetic-clipboard");
    await act(async () => vi.advanceTimersByTime(30000));
    expect(clipboard).toBe("synthetic-clipboard");
    await click("锁定密码库");
    expect(clipboard).toBe("synthetic-clipboard");
    await act(async () => window.dispatchEvent(new Event("pagehide")));
    await act(async () => root.render(<div>已关闭</div>));
    expect(clipboard).toBe("synthetic-clipboard");
    expect(requests).toEqual([[rows[0].entry_id, field]]);
  },
);
