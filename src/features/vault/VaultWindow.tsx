import { isTauri } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { flushSync } from "react-dom";
import { VaultEntries } from "./VaultEntries";
import { canonicalRevision } from "../../contracts/bridge";
import { useCallback, useEffect, useRef, useState } from "react";
import { LockSimple } from "@phosphor-icons/react/dist/csr/LockSimple";
import {
  createVaultBridge,
  desktopInvoke,
  BridgeError,
} from "../../contracts/bridge";
import type { VaultSummary, VaultMutation } from "../../contracts/domain";
import { EntryForm } from "./EntryForm";
import { vaultError } from "./errors";
import "./vault.css";
import { guardForm, newline, newlineMessage } from "./inputGuard";
export type VaultPort = ReturnType<typeof createVaultBridge>;
const nativePort = createVaultBridge(desktopInvoke);
/** Injection is for controlled UI tests. App always uses nativePort; no demo unlock. */
export type LockedSubscription = (callback: () => void) => Promise<() => void>;
const subscribeNativeLocked: LockedSubscription = async (callback) => {
  if (!isTauri()) throw new BridgeError("UNSUPPORTED");
  const remove = await listen("vault_locked", () => callback(), {
    target: { kind: "Webview", label: "vault" },
  });
  return () => {
    void Promise.resolve(remove()).catch(() => {});
  };
};
export type VisibilitySubscription = (
  callback: (visible: boolean) => void,
) => Promise<() => void>;
const noVisibility: VisibilitySubscription = async () => () => {};
const subscribeNativeVisibility: VisibilitySubscription = async (callback) => {
  const remove = await listen<{ visible: boolean; revision: string }>(
    "vault_view_visibility",
    (event) => {
      const { visible, revision } = event.payload;
      if (typeof visible !== "boolean") return;
      canonicalRevision(revision);
      flushSync(() => callback(visible));
      void desktopInvoke("ack_vault_view", { revision }).catch(() =>
        window.dispatchEvent(new Event("shixu-vault-view-failed")),
      );
    },
    { target: { kind: "Webview", label: "vault" } },
  );
  let redact: (() => void) | undefined;
  try {
    redact = await listen(
      "vault_view_redact",
      () => flushSync(() => callback(false)),
      { target: { kind: "Webview", label: "vault" } },
    );
    await desktopInvoke("ack_vault_view", { revision: null });
    return () => {
      remove();
      redact?.();
    };
  } catch (error) {
    remove();
    redact?.();
    throw error;
  }
};
export function VaultWindow({
  port = nativePort,
  subscribeLocked = subscribeNativeLocked,
  subscribeVisibility = port === nativePort
    ? subscribeNativeVisibility
    : noVisibility,
}: {
  port?: VaultPort;
  subscribeLocked?: LockedSubscription;
  subscribeVisibility?: VisibilitySubscription;
}) {
  const [state, setState] = useState<"locked" | "unlocking" | "unlocked">(
    "locked",
  );
  const [mode, setMode] = useState<"unlock" | "create" | "change">("unlock");
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const viewVisible = useRef(true);
  const [rows, setRows] = useState<VaultSummary[]>([]);
  const [search, setSearch] = useState("");
  const [message, setMessage] = useState("");
  const noticeTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const noticeSequence = useRef(0);
  const setNotice = useCallback((text: string, transient = false) => {
    const sequence = ++noticeSequence.current;
    if (noticeTimer.current) clearTimeout(noticeTimer.current);
    setMessage(text);
    if (transient)
      noticeTimer.current = setTimeout(() => {
        if (noticeSequence.current === sequence) setMessage("");
      }, 2500);
  }, []);
  useEffect(
    () => () => {
      if (noticeTimer.current) clearTimeout(noticeTimer.current);
    },
    [],
  );
  const [form, setForm] = useState<VaultSummary | "new" | null>(null);
  const [deletion, setDeletion] = useState<VaultSummary | null>(null);
  const [busy, setBusy] = useState(false);
  const [visible, setVisible] = useState<string | null>(null);
  const registrationReady = useRef(false);
  const [registrationAttempt, setRegistrationAttempt] = useState(0);
  const [registration, setRegistration] = useState<
    "pending" | "ready" | "failed"
  >("pending");
  const epoch = useRef(0);
  const mounted = useRef(true);
  const unlocked = useRef(false);
  const masterForm = useRef<HTMLFormElement>(null);
  // Ref detachment happens before passive unmount effects. Reset the actual
  // previous node before releasing it, including locked/change-form replacement.
  const masterGuard = useRef<(() => void) | null>(null);
  const attachMasterForm = useCallback((node: HTMLFormElement | null) => {
    masterGuard.current?.();
    masterForm.current?.reset();
    masterForm.current = node;
    masterGuard.current = node
      ? guardForm(node, ["master", "next", "confirm"], setNotice)
      : null;
  }, []);
  const ownedMaster = useRef<Uint8Array[]>([]);
  const secret = useRef<Uint8Array | null>(null);
  const revealSequence = useRef(0);
  const revealTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const clearReveal = useCallback(() => {
    revealSequence.current++;
    if (revealTimer.current) clearTimeout(revealTimer.current);
    secret.current?.fill(0);
    secret.current = null;
    setVisible(null);
  }, []);
  const clearLocal = useCallback(() => {
    epoch.current++;
    unlocked.current = false;
    clearReveal();
    ownedMaster.current.forEach((b) => b.fill(0));
    masterForm.current?.reset();
    setRows([]);
    setSelectedId(null);
    setSearch("");
    setForm(null);
    setDeletion(null);
    setState("locked");
    setMode("unlock");
    setBusy(false);
  }, [clearReveal]);
  const valid = (original: number) =>
    mounted.current && registrationReady.current && epoch.current === original;
  const touch = useCallback(() => {}, []);
  const lock = useCallback(() => {
    clearLocal();
    const original = epoch.current;
    setBusy(true);
    void port
      .vaultLock()
      .catch((error) => {
        if (mounted.current && epoch.current === original)
          setNotice(vaultError(error));
      })
      .finally(() => {
        if (mounted.current && epoch.current === original) setBusy(false);
      });
  }, [clearLocal, port]);
  useEffect(() => {
    mounted.current = true;
    const close = () => {
      clearLocal();
      void port.vaultLock().catch(() => {});
    };
    window.addEventListener("pagehide", close);
    return () => {
      mounted.current = false;
      epoch.current++;
      unlocked.current = false;
      revealSequence.current++;
      secret.current?.fill(0);
      secret.current = null;
      ownedMaster.current.forEach((b) => b.fill(0));
      if (revealTimer.current) clearTimeout(revealTimer.current);
      window.removeEventListener("pagehide", close);
      void port.vaultLock().catch(() => {});
    };
  }, [port, clearLocal]);
  useEffect(() => {
    registrationReady.current = false;
    setRegistration("pending");
    clearLocal();
    let active = true;
    const failed = () => {
      if (!active || !mounted.current) return;
      registrationReady.current = false;
      flushSync(() => {
        setRegistration("failed");
        clearLocal();
      });
      void port.vaultLock().catch(() => {});
      if (port === nativePort)
        void desktopInvoke("ack_vault_view", {
          revision: null,
          failed: true,
        }).catch(() => {});
    };
    window.addEventListener("shixu-vault-view-failed", failed);
    let unsubscribe: (() => void) | undefined;
    const removers: (() => void)[] = [];
    void (async () => {
      removers.push(
        await subscribeLocked(() => {
          if (active && mounted.current) clearLocal();
        }),
      );
      if (!active) {
        removers.forEach((fn) => fn());
        return;
      }
      removers.push(
        await subscribeVisibility((shown) => {
          if (!active || !mounted.current) return;
          viewVisible.current = shown;
          clearReveal();
          masterForm.current?.reset();
          ownedMaster.current.forEach((bytes) => bytes.fill(0));
          setForm(null);
          setDeletion(null);
          setMode("unlock");
          if (shown && unlocked.current) {
            const original = epoch.current;
            void port
              .vaultList()
              .then((result) => {
                if (valid(original) && unlocked.current) setRows(result);
              })
              .catch((error) => {
                if (valid(original)) {
                  clearLocal();
                  setNotice(vaultError(error));
                  void port.vaultLock().catch(() => {});
                }
              });
          }
        }),
      );
      return removers;
    })()
      .then((remove) => {
        if (!remove) return;
        if (active && mounted.current) {
          unsubscribe = () => remove.forEach((fn) => fn());
          registrationReady.current = true;
          setRegistration("ready");
        } else remove.forEach((fn) => fn());
      })
      .catch(() => {
        removers.forEach((fn) => fn());
        failed();
      });
    return () => {
      active = false;
      window.removeEventListener("shixu-vault-view-failed", failed);
      registrationReady.current = false;
      unsubscribe?.();
    };
  }, [
    subscribeLocked,
    subscribeVisibility,
    clearLocal,
    clearReveal,
    port,
    registrationAttempt,
  ]);
  async function authenticate(e: React.FormEvent) {
    e.preventDefault();
    if (!registrationReady.current) {
      clearLocal();
      return;
    }
    if (busy) return;
    const element = masterForm.current!;
    const get = (name: string) =>
      (element.elements.namedItem(name) as HTMLInputElement).value;
    const master = get("master");
    const next = mode === "change" ? get("next") : master;
    if (
      [master, next, mode !== "unlock" ? get("confirm") : ""].some((value) =>
        newline.test(value),
      )
    ) {
      setNotice(newlineMessage);
      element.reset();
      return;
    }
    if (mode !== "unlock" && next !== get("confirm")) {
      setNotice("两次主密码不一致。");
      element.reset();
      return;
    }
    const currentBytes = new TextEncoder().encode(master),
      nextBytes = new TextEncoder().encode(next);
    element.reset();
    ownedMaster.current = [currentBytes, nextBytes];
    const operation = mode;
    clearReveal();
    const original = ++epoch.current;
    setBusy(true);
    setNotice("");
    if (operation !== "change") {
      unlocked.current = false;
      setRows([]);
      setState("unlocking");
    }
    try {
      if (operation === "create") await port.vaultCreate(currentBytes);
      else if (operation === "change")
        await port.vaultChangeMaster(currentBytes, nextBytes);
      else await port.vaultUnlock(currentBytes);
      if (!valid(original)) return;
      if (operation === "change") {
        setMode("unlock");
        setNotice("主密码已更改。", true);
        return;
      }
      unlocked.current = true;
      setState("unlocked");
      touch();
      const result = await port.vaultList();
      if (valid(original) && unlocked.current) setRows(result);
    } catch (error) {
      if (valid(original)) {
        clearLocal();
        void port.vaultLock().catch(() => {});
        setNotice(vaultError(error));
      }
    } finally {
      currentBytes.fill(0);
      nextBytes.fill(0);
      if (valid(original)) {
        ownedMaster.current = [];
        setBusy(false);
      }
    }
  }
  async function mutate(value: VaultMutation) {
    if (!registrationReady.current || !unlocked.current)
      throw new BridgeError("LOCKED");
    clearReveal();
    const original = ++epoch.current;
    setBusy(true);
    setNotice("");
    try {
      const saved = await port.vaultApply(value);
      if (!valid(original) || !unlocked.current) return;
      setRows((previous) =>
        value.operation === "delete"
          ? previous.filter((r) => r.entry_id !== value.id)
          : value.operation === "update"
            ? previous.map((r) => (r.entry_id === value.id ? saved : r))
            : [...previous, saved],
      );
      if (value.operation !== "delete") setSelectedId(saved.entry_id);
      setForm(null);
      setDeletion(null);
      setNotice(value.operation === "delete" ? "已删除" : "已保存", true);
      touch();
    } catch (error) {
      if (valid(original)) {
        setNotice(vaultError(error, "save"));
        clearLocal();
        void port.vaultLock().catch(() => {});
      }
      throw error;
    } finally {
      if (valid(original)) setBusy(false);
    }
  }
  async function reveal(id: string) {
    if (!registrationReady.current || !unlocked.current || busy) return;
    clearReveal();
    const original = epoch.current,
      sequence = revealSequence.current;
    try {
      const bytes = await port.vaultReveal(id);
      if (
        !valid(original) ||
        !registrationReady.current ||
        !unlocked.current ||
        !viewVisible.current ||
        sequence !== revealSequence.current
      ) {
        bytes.fill(0);
        return;
      }
      secret.current = bytes;
      setVisible(id);
      touch();
      revealTimer.current = setTimeout(clearReveal, 15000);
    } catch (error) {
      if (valid(original)) {
        setNotice(vaultError(error));
        clearLocal();
        void port.vaultLock().catch(() => {});
      }
    }
  }
  async function copy(id: string, field: "account" | "password") {
    if (!registrationReady.current || !unlocked.current || busy) return;
    const original = epoch.current;
    try {
      await port.vaultCopy(id, field);
      if (valid(original) && unlocked.current) {
        setNotice(field === "account" ? "已复制账号" : "已复制密码", true);
        touch();
      }
    } catch (error) {
      if (valid(original)) {
        setNotice(vaultError(error));
        // Copy failures carry no trustworthy clipboard-only recovery proof.
        // Unknown transport errors are normalized to UNSUPPORTED by the bridge.
        clearLocal();
        void port.vaultLock().catch(() => {});
      }
    }
  }
  const filtered = rows.filter((r) =>
    `${r.channel}\n${r.account}`
      .toLocaleLowerCase()
      .includes(search.toLocaleLowerCase()),
  );
  const editing =
    mode === "change" ? (
      <form
        ref={attachMasterForm}
        className="vault-form"
        onSubmit={authenticate}
      >
        <h2>更改主密码</h2>
        <label>
          当前主密码
          <input
            aria-label="主密码"
            name="master"
            type="password"
            required
            autoComplete="off"
          />
        </label>
        <label>
          新主密码
          <input
            aria-label="新主密码"
            name="next"
            type="password"
            required
            autoComplete="off"
          />
        </label>
        <label>
          再次输入主密码
          <input
            aria-label="再次输入主密码"
            name="confirm"
            type="password"
            required
            autoComplete="off"
          />
        </label>
        <div className="vault-actions">
          <button
            className="primary"
            disabled={busy || registration !== "ready"}
          >
            保存主密码
          </button>
          <button
            type="button"
            onClick={() => {
              masterForm.current?.reset();
              setMode("unlock");
            }}
          >
            取消
          </button>
        </div>
      </form>
    ) : form ? (
      <section className="vault-editor">
        <h2>{form === "new" ? "新增条目" : "编辑条目"}</h2>
        <EntryForm
          key={form === "new" ? "new" : form.entry_id}
          initial={form === "new" ? undefined : form}
          onCancel={() => setForm(null)}
          onSave={(value) => {
            if (form === "new" && !value.password)
              throw new BridgeError("INVALID_INPUT");
            return mutate(
              form === "new"
                ? { ...value, operation: "create", password: value.password! }
                : {
                    operation: "update",
                    id: form.entry_id,
                    expected_revision: form.revision,
                    ...value,
                  },
            );
          }}
        />
      </section>
    ) : deletion ? (
      <section role="region" aria-label="确认删除" className="vault-confirm">
        <h2>确认删除</h2>
        <p>
          删除「{deletion.channel}」的账号「{deletion.account}」？
        </p>
        <div className="vault-actions">
          <button
            disabled={busy || registration !== "ready"}
            onClick={() =>
              void mutate({
                operation: "delete",
                id: deletion.entry_id,
                expected_revision: deletion.revision,
              }).catch(() => {})
            }
          >
            确认删除
          </button>
          <button
            disabled={busy || registration !== "ready"}
            onClick={() => setDeletion(null)}
          >
            取消
          </button>
        </div>
      </section>
    ) : undefined;
  const chosen =
    filtered.find((row) => row.entry_id === selectedId) ?? filtered[0];
  return (
    <main className="vault-shell">
      <header className="vault-header">
        <div className="vault-title">
          <h1>密码库</h1>
          <span
            className={`vault-state ${state === "unlocked" ? "unlocked" : ""}`}
          >
            {state === "unlocked" ? "已解锁" : "已锁定"}
          </span>
        </div>
        {state === "unlocked" && (
          <div className="vault-header-actions">
            <button
              disabled={busy}
              onClick={() => {
                clearReveal();
                setForm(null);
                setDeletion(null);
                setMode("change");
              }}
            >
              更改主密码
            </button>
            <button onClick={lock}>
              <LockSimple size={19} />
              锁定密码库
            </button>
          </div>
        )}
      </header>
      <div className="vault-body">
        {state !== "unlocked" ? (
          <section className="vault-locked">
            <div className="vault-emblem">
              <LockSimple size={42} />
            </div>
            <h1>{mode === "create" ? "新建密码库" : "密码库已锁定"}</h1>
            <p>解锁后查看账号与描述</p>
            <form
              ref={attachMasterForm}
              className="vault-form"
              onSubmit={authenticate}
            >
              <label>
                主密码
                <input
                  aria-label="主密码"
                  name="master"
                  type="password"
                  autoComplete="off"
                  required
                  disabled={busy || registration !== "ready"}
                />
              </label>
              {mode === "create" && (
                <label>
                  再次输入主密码
                  <input
                    aria-label="再次输入主密码"
                    name="confirm"
                    type="password"
                    autoComplete="off"
                    required
                    disabled={busy || registration !== "ready"}
                  />
                </label>
              )}
              <button
                className="primary"
                disabled={busy || registration !== "ready"}
              >
                {state === "unlocking"
                  ? "正在解锁…"
                  : mode === "create"
                    ? "创建密码库"
                    : "解锁密码库"}
              </button>
            </form>
            <div className="vault-actions">
              {mode === "create" ? (
                <button
                  disabled={busy || registration !== "ready"}
                  onClick={() => {
                    setMode("unlock");
                    setNotice("");
                  }}
                >
                  返回解锁
                </button>
              ) : (
                <>
                  <button
                    disabled={busy || registration !== "ready"}
                    onClick={() => {
                      masterForm.current?.reset();
                      setMode("create");
                      setNotice("");
                    }}
                  >
                    新建密码库
                  </button>
                </>
              )}
            </div>
            {mode === "create" && (
              <p className="vault-help">主密码无法找回，请妥善保管。</p>
            )}
            {new URLSearchParams(location.search).get("fictional") === "1" && (
              <p className="vault-availability">
                当前是独立的虚构密码库，请仅使用测试账号和密码。
              </p>
            )}
            {state === "unlocking" && (
              <button onClick={lock}>锁定密码库</button>
            )}
          </section>
        ) : (
          <VaultEntries
            rows={filtered}
            selectedId={selectedId}
            search={search}
            busy={busy || registration !== "ready"}
            visiblePassword={
              chosen?.entry_id === visible && secret.current
                ? new TextDecoder().decode(secret.current)
                : null
            }
            onSearch={(value) => {
              clearReveal();
              setSearch(value);
              setForm(null);
              setDeletion(null);
            }}
            onSelect={(row) => {
              clearReveal();
              setSelectedId(row.entry_id);
              setForm(null);
              setDeletion(null);
              setMode("unlock");
            }}
            onNew={() => {
              clearReveal();
              setMode("unlock");
              setDeletion(null);
              setForm("new");
            }}
            onEdit={(row) => {
              clearReveal();
              setForm(row);
            }}
            onDelete={(row) => {
              clearReveal();
              setDeletion(row);
            }}
            onReveal={(row) =>
              visible === row.entry_id
                ? clearReveal()
                : void reveal(row.entry_id)
            }
            onCopy={(row, field) => void copy(row.entry_id, field)}
          >
            {editing}
          </VaultEntries>
        )}
      </div>
      {registration === "failed" && (
        <button
          className="vault-retry"
          onClick={() => setRegistrationAttempt((value) => value + 1)}
        >
          重试密码库
        </button>
      )}
      <p role="status" className="vault-message">
        {registration === "pending"
          ? "正在准备密码库。"
          : registration === "failed"
            ? "密码库暂不可用，请重新进入。"
            : message}
      </p>
    </main>
  );
}
