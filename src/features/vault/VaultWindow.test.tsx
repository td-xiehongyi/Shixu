// @vitest-environment jsdom
import { act, StrictMode } from "react";
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
  description: `虚构描述${n}\n第二行`,
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
    vaultActivity: async () => {},
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
const readySubscription = async () => () => {};
it("pointer_reveal_stays_unlocked_when_an_obsolete_idle_request_would_conflict", async () => {
  const lock = vi.fn(async () => {});
  await render(
    port({
      vaultActivity: async () => {
        throw new BridgeError("CONFLICT");
      },
      vaultLock: lock,
    }),
  );
  await unlock();
  lock.mockClear();
  await act(async () => {
    const reveal = button("显示密码");
    reveal.dispatchEvent(new Event("pointerdown", { bubbles: true }));
    reveal.click();
  });
  expect(host.querySelector(".vault-state")?.textContent).toBe("已解锁");
  expect(host.querySelector(".vault-details code")?.textContent).toBe(
    "synthetic-reveal",
  );
  expect(lock).not.toHaveBeenCalled();
});
it("entry_form_preserves_optional_multiline_description", async () => {
  let saved: unknown;
  await act(async () =>
    root.render(
      <EntryForm
        onSave={async (value) => {
          saved = {
            ...value,
            password: new TextDecoder().decode(value.password),
          };
        }}
      />,
    ),
  );
  await fill("渠道", "虚构渠道");
  await fill("账号", "fictional@example.invalid");
  await fill("密码", "fictional-only-password");
  await fill("描述", "  用途说明\n第二行  ");
  await click("保存条目");
  expect(saved).toEqual({
    channel: "虚构渠道",
    account: "fictional@example.invalid",
    password: "fictional-only-password",
    description: "  用途说明\n第二行  ",
  });
});
async function render(p: VaultPort) {
  await act(async () =>
    root.render(<VaultWindow port={p} subscribeLocked={readySubscription} />),
  );
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
  const input = [...host.querySelectorAll("input, textarea")].find(
    (e) => e.getAttribute("aria-label") === label,
  );
  expect(input, `field ${label}`).toBeTruthy();
  await act(async () => {
    Object.getOwnPropertyDescriptor(
      input instanceof HTMLTextAreaElement
        ? HTMLTextAreaElement.prototype
        : HTMLInputElement.prototype,
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
it("entry_fields_include_optional_description", async () => {
  await act(async () => root.render(<EntryForm onSave={async () => {}} />));
  expect(
    [...host.querySelectorAll("label")].map((l) => l.firstChild?.textContent),
  ).toEqual(["渠道", "账号", "密码", "描述"]);
});
it("locked_hides_accounts", async () => {
  await render(port());
  expect(host.textContent).not.toContain("虚构账号");
  await unlock();
  expect(host.textContent).toContain("虚构账号1");
  expect(host.textContent).toContain("虚构描述1\n第二行");
  await click("锁定密码库");
  expect(host.textContent).not.toContain("虚构账号");
  expect(host.textContent).not.toContain("虚构描述");
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
    host
      .querySelectorAll<HTMLButtonElement>("button[data-entry-id]")[1]
      .click(),
  );
  await act(async () =>
    host.querySelector<HTMLButtonElement>("button[data-edit]")!.click(),
  );
  await click("修改密码");
  await fill("密码", "  synthetic-password  ");
  await click("保存条目");
  expect(sent?.operation).toBe("update");
  if (sent?.operation === "update") {
    expect(sent.id).toBe(rows[1].entry_id);
    expect(sent.expected_revision).toBe(rows[1].revision);
    expect(sent.password?.every((b) => b === 0)).toBe(true);
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
it("inactivity_keeps_vault_accessible_until_manual_lock", async () => {
  await render(port());
  await unlock();
  await act(async () => vi.advanceTimersByTime(299999));
  expect(host.textContent).toContain("虚构账号");
  await act(async () => vi.advanceTimersByTime(86400000));
  expect(host.textContent).toContain("虚构账号");
  await click("锁定密码库");
  expect(host.textContent).not.toContain("虚构账号");
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
  expect(host.textContent).toContain("无法找回");
  await click("返回解锁");
  expect(host.textContent).not.toContain("忘记主密码");
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
  expect(host.textContent).not.toContain("已保存");
  await act(async () => d.resolve(rows[0]));
  expect(host.textContent).toContain("已保存");
  await click("删除条目");
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
  expect(host.textContent).not.toContain("已保存");
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
  "copy_%s_shows_concise_notice_without_automatic_cleanup",
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
    expect(host.querySelector('[role="status"]')?.textContent).toBe(
      field === "password" ? "已复制密码" : "已复制账号",
    );
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
it("rejects_multiline_paste_before_single_line_normalization", async () => {
  await render(port());
  const input = host.querySelector<HTMLInputElement>('[name="master"]')!;
  input.value = "previous";
  const event = new Event("paste", { bubbles: true, cancelable: true });
  Object.defineProperty(event, "clipboardData", {
    value: { getData: () => "secret\r\nnext" },
  });
  await act(async () => input.dispatchEvent(event));
  expect(event.defaultPrevented).toBe(true);
  expect(input.value).toBe("");
  expect(host.textContent).toContain("不允许换行");
});
it("native_conflict_clears_unlocked_rows_and_reveal", async () => {
  await render(
    port({
      vaultApply: async () => {
        throw new BridgeError("CONFLICT");
      },
    }),
  );
  await unlock();
  await click("显示密码");
  await click("新增条目");
  await fill("渠道", "test");
  await fill("账号", "test");
  await fill("密码", "test");
  await click("保存条目");
  expect(host.textContent).not.toContain("虚构账号");
  expect(host.textContent).toContain("密码库已锁定");
});
it("direct_bridge_rejects_newlines_without_invoking", async () => {
  const invoke = vi.fn(async () => null);
  const bridge = createVaultBridge(invoke);
  for (const newline of ["\r", "\n", "\u0085", "\u2028", "\u2029"])
    await expect(
      bridge.vaultCreate(
        Uint8Array.from(new TextEncoder().encode(`a${newline}b`)),
      ),
    ).rejects.toMatchObject({ code: "INVALID_INPUT" });
  expect(invoke).not.toHaveBeenCalled();
});
it("pointer_and_keyboard_do_not_queue_idle_heartbeats_for_process_lifetime_unlock", async () => {
  const activity = vi.fn(async () => {});
  await render(port({ vaultActivity: activity } as Partial<VaultPort>));
  await unlock();
  activity.mockClear();
  await act(async () => {
    vi.advanceTimersByTime(1000);
  });
  expect(activity).not.toHaveBeenCalled();
  await act(async () =>
    host
      .querySelector("main")!
      .dispatchEvent(new Event("pointerdown", { bubbles: true })),
  );
  await act(async () =>
    host
      .querySelector("main")!
      .dispatchEvent(new Event("keydown", { bubbles: true })),
  );
  expect(activity).not.toHaveBeenCalled();
});
it("list_decode_failure_revokes_native_session_after_authentication", async () => {
  const lockNative = vi.fn(async () => {});
  await render(
    port({
      vaultLock: lockNative,
      vaultList: async () => {
        throw new BridgeError("INVALID_INPUT");
      },
    }),
  );
  await unlock();
  expect(host.textContent).toContain("密码库已锁定");
  expect(lockNative).toHaveBeenCalledTimes(1);
});
it("decodes_backend_legal_large_fields_without_narrowing_storage_text", async () => {
  const bridge = createVaultBridge(async () => [
    { ...rows[0], channel: "a".repeat(65536), account: "b".repeat(65536) },
  ]);
  const result = await bridge.vaultList();
  expect(result[0].channel.length).toBe(65536);
});
it("all_sensitive_fields_reject_original_paste_drop_and_beforeinput", async () => {
  await render(port());
  await click("新建密码库");
  const rejectEvents = async (names: string[]) => {
    for (const name of names)
      for (const type of ["paste", "drop", "beforeinput"])
        for (const newline of ["\r\n", "\u0085", "\u2028", "\u2029"]) {
          const input = host.querySelector<HTMLInputElement>(
            `[name="${name}"]`,
          )!;
          input.value = "previous";
          const event = new Event(type, { bubbles: true, cancelable: true });
          const getData = () => `synthetic${newline}payload`;
          Object.defineProperty(
            event,
            type === "paste"
              ? "clipboardData"
              : type === "drop"
                ? "dataTransfer"
                : "data",
            { value: type === "beforeinput" ? getData() : { getData } },
          );
          await act(async () => input.dispatchEvent(event));
          expect(event.defaultPrevented).toBe(true);
          expect(input.value).toBe("");
          expect(host.textContent).not.toContain("synthetic");
        }
  };
  await rejectEvents(["master", "confirm"]);
  await click("返回解锁");
  await unlock();
  await click("更改主密码");
  await rejectEvents(["master", "next", "confirm"]);
  await act(async () => root.render(<EntryForm onSave={async () => {}} />));
  await rejectEvents(["password"]);
});
it("entry_form_preserves_channel_newlines_whitespace_unicode_and_nul", async () => {
  let saved: { channel: string; account: string; password: string } | undefined;
  const channel = " line\n渠道\0 ",
    account = ' 用户/"\0 ',
    password = ' 密钥/"\0 ';
  await act(async () =>
    root.render(
      <EntryForm
        initial={{ ...rows[0], channel, account }}
        onSave={async (value) => {
          saved = {
            ...value,
            password: new TextDecoder().decode(value.password),
          };
        }}
      />,
    ),
  );
  await click("修改密码");
  await fill("密码", password);
  await click("保存条目");
  expect(saved).toEqual({
    channel,
    account,
    description: rows[0].description,
    password,
  });
});
it("bridge_secret_validation_is_structural_and_rejects_all_secret_newline_paths", async () => {
  const invoke = vi.fn(async () => null),
    bridge = createVaultBridge(invoke);
  const encode = (s: string) => Uint8Array.from(new TextEncoder().encode(s));
  const spy = vi.spyOn(JSON, "stringify").mockImplementation(() => {
    throw new Error("secret serialization forbidden");
  });
  try {
    await bridge.vaultCreate(encode(" valid 密码 "));
    expect(invoke).toHaveBeenCalledTimes(1);
    invoke.mockClear();
    for (const newline of ["\r", "\n", "\u0085", "\u2028", "\u2029"]) {
      const bad = encode(`a${newline}b`);
      await expect(bridge.vaultUnlock(bad)).rejects.toMatchObject({
        code: "INVALID_INPUT",
      });
      await expect(
        bridge.vaultChangeMaster(encode("ok"), bad),
      ).rejects.toMatchObject({ code: "INVALID_INPUT" });
      await expect(
        bridge.vaultApply({
          operation: "create",
          channel: "ch",
          account: `a${newline}b`,
          password: bad,
        }),
      ).rejects.toMatchObject({ code: "INVALID_INPUT" });
      await expect(
        bridge.vaultApply({
          operation: "create",
          channel: "ch",
          account: "ok",
          password: bad,
        }),
      ).rejects.toMatchObject({ code: "INVALID_INPUT" });
    }
    expect(invoke).not.toHaveBeenCalled();
  } finally {
    spy.mockRestore();
  }
});
it("unchanged_existing_channel_preserves_cr_and_crlf_when_editing_password", async () => {
  const channel = " 渠道\r单独\r\n换行 ";
  let savedChannel = "";
  await act(async () =>
    root.render(
      <EntryForm
        initial={{ ...rows[0], channel }}
        onSave={async (value) => {
          savedChannel = value.channel;
        }}
      />,
    ),
  );
  await click("修改密码");
  await fill("密码", "changed password");
  await click("保存条目");
  expect(savedChannel).toBe(channel);
});
it("detached_entry_controls_clear_existing_account_and_channel", async () => {
  await act(async () =>
    root.render(<EntryForm initial={rows[0]} onSave={async () => {}} />),
  );
  const controls = [
    ...host.querySelectorAll<HTMLInputElement | HTMLTextAreaElement>(
      "input, textarea",
    ),
  ];
  await act(async () => root.render(<div>closed</div>));
  expect(controls.map((input) => input.value)).toEqual(["", "", ""]);
});

it("strictmode_replay_restores_defaults_preserves_exact_edit_and_clears_detached_controls", async () => {
  const channel = ' 渠道\r单独\r\n换行/"\u0000 🗝️ ';
  const account = ' 用户/"\u0000 🗝️ ';
  const password = ' 新密码/"\u0000 🗝️ ';
  const description = " 保留描述\r\n第二行 ";
  let saved: { channel: string; account: string; password: string } | undefined;
  await act(async () =>
    root.render(
      <StrictMode>
        <EntryForm
          initial={{ ...rows[0], channel, account, description }}
          onSave={async (value) => {
            saved = {
              ...value,
              password: new TextDecoder().decode(value.password),
            };
          }}
        />
      </StrictMode>,
    ),
  );
  const channelInput =
    host.querySelector<HTMLTextAreaElement>('[name="channel"]')!;
  const accountInput =
    host.querySelector<HTMLInputElement>('[name="account"]')!;
  expect(channelInput.value).toBe(channel.replace(/\r\n?/g, "\n"));
  expect(accountInput.value).toBe(account);
  await click("修改密码");
  await fill("密码", password);
  await click("保存条目");
  expect(saved).toEqual({ channel, account, description, password });
  const descriptionInput = host.querySelector<HTMLTextAreaElement>(
    '[name="description"]',
  )!;
  const controls = [
    ...host.querySelectorAll<HTMLInputElement | HTMLTextAreaElement>(
      "input, textarea",
    ),
  ];
  await act(async () => root.render(<div>closed</div>));
  expect(controls.map((input) => input.value)).toEqual(["", "", "", ""]);
  expect(descriptionInput.defaultValue).toBe("");
  expect(descriptionInput.textContent).toBe("");
});
it("account_all_newlines_are_preserved_on_strictmode_password_only_edit_and_bridge", async () => {
  const account = ' 用户\r单独\r\n行\n\u0085\u2028\u2029/"\0 🗝️ ';
  let savedAccount = "";
  await act(async () =>
    root.render(
      <StrictMode>
        <EntryForm
          initial={{ ...rows[0], account }}
          onSave={async (value) => {
            savedAccount = value.account;
          }}
        />
      </StrictMode>,
    ),
  );
  const accountInput =
    host.querySelector<HTMLTextAreaElement>('[name="account"]')!;
  expect(accountInput.value).toBe(account.replace(/\r\n?/g, "\n"));
  for (const type of ["paste", "drop", "beforeinput"]) {
    const event = new Event(type, { bubbles: true, cancelable: true });
    Object.defineProperty(
      event,
      type === "paste"
        ? "clipboardData"
        : type === "drop"
          ? "dataTransfer"
          : "data",
      { value: type === "beforeinput" ? account : { getData: () => account } },
    );
    await act(async () => accountInput.dispatchEvent(event));
    expect(event.defaultPrevented).toBe(false);
  }
  await click("修改密码");
  await fill("密码", "new password");
  await click("保存条目");
  expect(savedAccount).toBe(account);
  const invoke = vi.fn(async () => ({ ...rows[0], account }));
  const bridge = createVaultBridge(invoke);
  const result = await bridge.vaultApply({
    operation: "create",
    channel: "ch",
    account,
    password: Uint8Array.from(new TextEncoder().encode("pw")),
  });
  expect(result.account).toBe(account);
  expect(invoke).toHaveBeenCalledTimes(1);
  await act(async () => root.render(<div>closed</div>));
  expect(accountInput.value).toBe("");
});
it("native lock event clears revealed bytes and rows without interaction", async () => {
  const callbacks: (() => void)[] = [];
  const removed = vi.fn();
  const bytes = new TextEncoder().encode("synthetic-visible-native");
  await act(async () =>
    root.render(
      <StrictMode>
        <VaultWindow
          port={port({ vaultReveal: async () => bytes })}
          subscribeLocked={async (callback) => {
            callbacks.push(callback);
            return removed;
          }}
        />
      </StrictMode>,
    ),
  );
  await unlock();
  await click("显示密码");
  expect(host.textContent).toContain("synthetic-visible-native");
  await act(async () => callbacks.at(-1)!());
  expect(host.textContent).toContain("密码库已锁定");
  expect(host.textContent).not.toContain("虚构账号");
  expect(host.textContent).not.toContain("synthetic-visible-native");
  expect([...bytes]).toEqual([...bytes].map(() => 0));
  expect(removed).toHaveBeenCalledTimes(1); // StrictMode's retired listener.
  await unlock();
  await act(async () => callbacks[0]()); // retired callback has no authority.
  expect(host.textContent).toContain("虚构账号");
  await click("更改主密码");
  await fill("主密码", "owned synthetic current");
  await fill("新主密码", "owned synthetic next");
  const retained = [
    ...host.querySelectorAll<HTMLInputElement>('input[type="password"]'),
  ];
  await act(async () => callbacks.at(-1)!());
  expect(host.textContent).toContain("密码库已锁定");
  expect(retained.every((input) => input.value === "")).toBe(true);
});
it("late native listener registration is retired after unmount", async () => {
  let complete!: (remove: () => void) => void;
  let callback!: () => void;
  const remove = vi.fn();
  const p = port();
  await act(async () =>
    root.render(
      <VaultWindow
        port={p}
        subscribeLocked={(onLock) => {
          callback = onLock;
          return new Promise((resolve) => {
            complete = resolve;
          });
        }}
      />,
    ),
  );
  await fill("主密码", "owned synthetic input");
  const retained = host.querySelector<HTMLInputElement>(
    'input[name="master"]',
  )!;
  await act(async () => root.render(null));
  expect(retained.value).toBe("");
  await act(async () => {
    complete(remove);
  });
  expect(remove).toHaveBeenCalledTimes(1);
  await act(async () => callback());
  expect(host.textContent).toBe("");
});

for (const status of ["pending", "failed"] as const) {
  it(`subscription_${status}_refuses_even_programmatic_authentication`, async () => {
    const unlockSpy = vi.fn(async () => {});
    const subscribe = () =>
      status === "pending"
        ? new Promise<() => void>(() => {})
        : Promise.reject(new Error("synthetic listener failure"));
    await act(async () =>
      root.render(
        <VaultWindow
          port={port({ vaultUnlock: unlockSpy })}
          subscribeLocked={subscribe}
        />,
      ),
    );
    await fill("主密码", "synthetic-master");
    await act(async () =>
      host
        .querySelector("form")!
        .dispatchEvent(
          new Event("submit", { bubbles: true, cancelable: true }),
        ),
    );
    expect(unlockSpy).not.toHaveBeenCalled();
    expect(host.textContent).toContain(
      status === "pending" ? "正在准备密码库" : "密码库暂不可用",
    );
    expect(
      (host.querySelector('[aria-label="主密码"]') as HTMLInputElement).value,
    ).toBe("");
    expect(button("解锁密码库").disabled).toBe(true);
  });
}
it("subscription_success_admits_authentication_and_retirement_revokes_it", async () => {
  const registration = deferred<() => void>();
  const subscribe = () => registration.promise;
  const unlockSpy = vi.fn(async () => {});
  const p = port({ vaultUnlock: unlockSpy });
  await act(async () =>
    root.render(<VaultWindow port={p} subscribeLocked={subscribe} />),
  );
  expect(button("解锁密码库").disabled).toBe(true);
  const remove = vi.fn();
  await act(async () => registration.resolve(remove));
  await unlock();
  expect(unlockSpy).toHaveBeenCalledTimes(1);
  expect(host.textContent).toContain("虚构账号1");
  const replacement = () => new Promise<() => void>(() => {});
  await act(async () =>
    root.render(<VaultWindow port={p} subscribeLocked={replacement} />),
  );
  expect(remove).toHaveBeenCalledTimes(1);
  expect(host.textContent).not.toContain("虚构账号1");
  await act(async () =>
    host
      .querySelector("form")!
      .dispatchEvent(new Event("submit", { bubbles: true, cancelable: true })),
  );
  expect(unlockSpy).toHaveBeenCalledTimes(1);
});

it("failed_subscription_stays_unavailable_until_deliberate_successful_registration", async () => {
  const p = port({ vaultUnlock: vi.fn(async () => {}) });
  const failed = () => Promise.reject(new Error("synthetic failure"));
  await act(async () =>
    root.render(<VaultWindow port={p} subscribeLocked={failed} />),
  );
  await act(async () =>
    host
      .querySelector("form")!
      .dispatchEvent(new Event("submit", { bubbles: true, cancelable: true })),
  );
  expect(p.vaultUnlock).not.toHaveBeenCalled();
  await act(async () =>
    root.render(<VaultWindow port={p} subscribeLocked={readySubscription} />),
  );
  await unlock();
  expect(p.vaultUnlock).toHaveBeenCalledTimes(1);
});

it("selected_details_hide_other_descriptions_and_switch_hides_password", async () => {
  await render(port());
  await unlock();
  expect(host.querySelector(".vault-details")?.textContent).toContain(
    "虚构描述1",
  );
  expect(host.querySelector(".vault-details")?.textContent).not.toContain(
    "虚构描述2",
  );
  await click("显示密码");
  await act(async () =>
    host.querySelectorAll<HTMLButtonElement>("[data-entry-id]")[1].click(),
  );
  expect(host.querySelector(".vault-details")?.textContent).toContain(
    "虚构描述2",
  );
  expect(host.textContent).not.toContain("synthetic-reveal");
});
it("visibility_redacts_without_revoking_and_late_reveal_is_wiped", async () => {
  let visibility!: (visible: boolean) => void;
  const d = deferred<Uint8Array>();
  const unlockSpy = vi.fn(async () => {}),
    lockSpy = vi.fn(async () => {});
  await act(async () =>
    root.render(
      <VaultWindow
        port={port({
          vaultReveal: () => d.promise,
          vaultUnlock: unlockSpy,
          vaultLock: lockSpy,
        })}
        subscribeLocked={readySubscription}
        subscribeVisibility={async (callback) => {
          visibility = callback;
          return () => {};
        }}
      />,
    ),
  );
  await unlock();
  await click("显示密码");
  await act(async () => visibility(false));
  const bytes = new TextEncoder().encode("hidden-late-secret");
  await act(async () => d.resolve(bytes));
  expect(bytes.every((b) => b === 0)).toBe(true);
  await act(async () => visibility(true));
  expect(host.textContent).toContain("虚构账号1");
  expect(host.textContent).not.toContain("hidden-late-secret");
  expect(unlockSpy).toHaveBeenCalledTimes(1);
  expect(lockSpy).not.toHaveBeenCalled();
});

it("search_replaces_selection_and_clears_password_and_old_description", async () => {
  await render(port());
  await unlock();
  await click("显示密码");
  await fill("搜索渠道或账号", "虚构账号2");
  const detail = () => host.querySelector(".vault-details")!.textContent;
  expect(detail()).toContain("虚构描述2");
  expect(detail()).not.toContain("虚构描述1");
  expect(detail()).not.toContain("synthetic-reveal");
  await fill("搜索渠道或账号", "no matches");
  expect(detail()).not.toContain("虚构描述");
  expect(detail()).toContain("选择或新增");
});
it("hidden_editor_wipes_unsaved_input_and_pending_save_only_updates_rows", async () => {
  let visibility!: (visible: boolean) => void;
  const d = deferred<VaultSummary>();
  await act(async () =>
    root.render(
      <VaultWindow
        port={port({ vaultApply: () => d.promise })}
        subscribeLocked={readySubscription}
        subscribeVisibility={async (cb) => {
          visibility = cb;
          return () => {};
        }}
      />,
    ),
  );
  await unlock();
  await click("新增条目");
  await fill("渠道", "新的虚构渠道");
  await fill("账号", "pending@example.invalid");
  await fill("密码", "unsaved-password");
  await fill("描述", "unsaved-description");
  const password = host.querySelector<HTMLInputElement>('[aria-label="密码"]')!;
  await click("保存条目");
  await act(async () => visibility(false));
  expect(password.value).toBe("");
  expect(host.querySelector(".vault-editor")).toBeNull();
  await act(async () =>
    d.resolve({
      ...rows[0],
      entry_id: "22222222-2222-4222-8222-222222222222",
      channel: "新的虚构渠道",
      account: "pending@example.invalid",
    }),
  );
  expect(host.querySelector(".vault-editor")).toBeNull();
  expect(host.textContent).toContain("pending@example.invalid");
  await act(async () => visibility(true));
  expect(host.querySelector(".vault-state")?.textContent).toBe("已解锁");
});
it("partial_visibility_registration_failure_removes_locked_listener", async () => {
  const removed = vi.fn();
  await act(async () =>
    root.render(
      <VaultWindow
        port={port()}
        subscribeLocked={async () => removed}
        subscribeVisibility={async () => {
          throw Error("listener failed");
        }}
      />,
    ),
  );
  expect(removed).toHaveBeenCalledTimes(1);
  expect(button("解锁密码库").disabled).toBe(true);
});

it("failed_registration_can_be_retried_in_place", async () => {
  let attempts = 0;
  const subscribe = async () => {
    if (++attempts === 1) throw Error("transient failure");
    return () => {};
  };
  await act(async () =>
    root.render(<VaultWindow port={port()} subscribeLocked={subscribe} />),
  );
  await click("重试密码库");
  await unlock();
  expect(host.textContent).toContain("虚构账号1");
  expect(attempts).toBe(2);
});
it("editing_metadata_keeps_password_inside_engine", async () => {
  const reveal = vi.fn(async () => new Uint8Array([1]));
  let mutation: VaultMutation | undefined;
  await render(
    port({
      vaultReveal: reveal,
      vaultApply: async (m) => {
        mutation = m;
        return rows[0];
      },
    }),
  );
  await unlock();
  await click("编辑");
  expect(host.querySelector('[name="password"]')).toBeNull();
  await fill("描述", "只更新描述");
  await click("保存条目");
  expect(mutation).toMatchObject({
    operation: "update",
    description: "只更新描述",
  });
  expect(mutation).not.toHaveProperty("password");
  expect(reveal).not.toHaveBeenCalled();
  expect(host.querySelector(".vault-state")?.textContent).toBe("已解锁");
});
it("explicit_password_change_requires_value_and_cancel_preserves_original", async () => {
  const saved = vi.fn(async (_value: { password?: Uint8Array }) => {});
  await act(async () =>
    root.render(<EntryForm initial={rows[0]} onSave={saved} />),
  );
  await click("修改密码");
  await click("保存条目");
  expect(saved).not.toHaveBeenCalled();
  await fill("密码", "temporary-password");
  const detached = host.querySelector<HTMLInputElement>('[name="password"]')!;
  await click("取消修改密码");
  expect(detached.value).toBe("");
  await click("保存条目");
  expect(saved.mock.calls[0]?.[0]).not.toHaveProperty("password");
});
it("concise_feedback_expires_but_does_not_lock_or_clear_clipboard", async () => {
  const lock = vi.fn(async () => {});
  await render(port({ vaultLock: lock }));
  await unlock();
  await click("复制密码");
  expect(host.querySelector('[role="status"]')?.textContent).toBe("已复制密码");
  const calls = lock.mock.calls.length;
  await act(async () => vi.advanceTimersByTime(3000));
  expect(host.querySelector('[role="status"]')?.textContent).toBe("");
  expect(lock.mock.calls.length).toBe(calls);
  expect(host.querySelector(".vault-state")?.textContent).toBe("已解锁");
});
it("recovery_warning_only_on_creation_and_no_repeated_footer", async () => {
  await render(port());
  expect(host.textContent).not.toContain("忘记主密码");
  expect(host.querySelector(".vault-footer")).toBeNull();
  expect(host.textContent).not.toContain("主密码无法找回");
  await click("新建密码库");
  expect(host.textContent).toContain("主密码无法找回，请妥善保管。");
});

it("update_omits_password_without_reveal_and_empty_replacement_is_rejected", async () => {
  const invoke = vi.fn(async () => rows[0]);
  const bridge = createVaultBridge(invoke);
  const update = {
    operation: "update" as const,
    id: rows[0].entry_id,
    expected_revision: rows[0].revision,
    channel: "changed",
    account: "account",
    description: "new",
  };
  await bridge.vaultApply(update);
  expect(invoke.mock.calls[0]).toEqual(["vault_apply", { mutation: update }]);
  await expect(
    bridge.vaultApply({ ...update, password: new Uint8Array() }),
  ).rejects.toMatchObject({ code: "INVALID_INPUT" });
  expect(invoke).toHaveBeenCalledTimes(1);
});
it("new_error_is_not_erased_by_previous_success_notice_timer", async () => {
  await render(
    port({
      vaultCopy: async (_id, field) => {
        if (field === "account") throw new BridgeError("DISCONNECTED");
      },
    }),
  );
  await unlock();
  await click("复制密码");
  await click("复制账号");
  await act(async () => vi.advanceTimersByTime(3000));
  expect(host.querySelector('[role="status"]')?.textContent).toContain(
    "服务未连接",
  );
});
