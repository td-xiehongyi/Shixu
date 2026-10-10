import { useCallback, useEffect, useRef, useState } from "react";
import { LockSimple } from "@phosphor-icons/react/dist/csr/LockSimple";
import { ShieldCheck } from "@phosphor-icons/react/dist/csr/ShieldCheck";
import {
  createVaultBridge,
  desktopInvoke,
  BridgeError,
} from "../../contracts/bridge";
import type { VaultSummary, VaultMutation } from "../../contracts/domain";
import { EntryForm } from "./EntryForm";
import { vaultError } from "./errors";
import "./vault.css";
export type VaultPort = ReturnType<typeof createVaultBridge>;
const nativePort = createVaultBridge(desktopInvoke);
/** Injection is for controlled UI tests. App always uses nativePort; no demo unlock. */
export function VaultWindow({ port = nativePort }: { port?: VaultPort }) {
  const [state, setState] = useState<"locked" | "unlocking" | "unlocked">(
    "locked",
  );
  const [mode, setMode] = useState<"unlock" | "create" | "change">("unlock");
  const [rows, setRows] = useState<VaultSummary[]>([]);
  const [search, setSearch] = useState("");
  const [message, setMessage] = useState("");
  const [forgot, setForgot] = useState(false);
  const [form, setForm] = useState<VaultSummary | "new" | null>(null);
  const [deletion, setDeletion] = useState<VaultSummary | null>(null);
  const [busy, setBusy] = useState(false);
  const [visible, setVisible] = useState<string | null>(null);
  const epoch = useRef(0);
  const mounted = useRef(true);
  const unlocked = useRef(false);
  const masterForm = useRef<HTMLFormElement>(null);
  // Ref detachment happens before passive unmount effects. Reset the actual
  // previous node before releasing it, including locked/change-form replacement.
  const attachMasterForm = useCallback((node: HTMLFormElement | null) => {
    masterForm.current?.reset();
    masterForm.current = node;
  }, []);
  const ownedMaster = useRef<Uint8Array[]>([]);
  const secret = useRef<Uint8Array | null>(null);
  const revealSequence = useRef(0);
  const revealTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const idleTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const lockRef = useRef<() => void>(() => {});
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
    if (idleTimer.current) clearTimeout(idleTimer.current);
    ownedMaster.current.forEach((b) => b.fill(0));
    masterForm.current?.reset();
    setRows([]);
    setSearch("");
    setForm(null);
    setDeletion(null);
    setState("locked");
    setMode("unlock");
    setBusy(false);
  }, [clearReveal]);
  const valid = (original: number) =>
    mounted.current && epoch.current === original;
  const touch = useCallback(() => {
    if (!unlocked.current) return;
    if (idleTimer.current) clearTimeout(idleTimer.current);
    idleTimer.current = setTimeout(() => lockRef.current(), 300000);
  }, []);
  const lock = useCallback(() => {
    clearLocal();
    const original = epoch.current;
    setBusy(true);
    void port
      .vaultLock()
      .catch((error) => {
        if (mounted.current && epoch.current === original)
          setMessage(vaultError(error));
      })
      .finally(() => {
        if (mounted.current && epoch.current === original) setBusy(false);
      });
  }, [clearLocal, port]);
  useEffect(() => {
    lockRef.current = lock;
  }, [lock]);
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
      if (idleTimer.current) clearTimeout(idleTimer.current);
      window.removeEventListener("pagehide", close);
      void port.vaultLock().catch(() => {});
    };
  }, [port, clearLocal]);
  async function authenticate(e: React.FormEvent) {
    e.preventDefault();
    if (busy) return;
    const element = masterForm.current!;
    const get = (name: string) =>
      (element.elements.namedItem(name) as HTMLInputElement).value;
    const master = get("master");
    const next = mode === "change" ? get("next") : master;
    if (mode !== "unlock" && next !== get("confirm")) {
      setMessage("两次主密码不一致。");
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
    setMessage("");
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
        setMessage("主密码已更改。");
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
        setMessage(vaultError(error));
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
    if (!unlocked.current) throw new BridgeError("LOCKED");
    clearReveal();
    const original = ++epoch.current;
    setBusy(true);
    setMessage("");
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
      setForm(null);
      setDeletion(null);
      setMessage(
        value.operation === "delete" ? "条目已删除。" : "条目已保存。",
      );
      touch();
    } catch (error) {
      if (valid(original)) {
        setMessage(vaultError(error));
        if (
          !(error instanceof BridgeError) ||
          !["CONFLICT", "INVALID_INPUT"].includes(error.code)
        )
          clearLocal();
      }
      throw error;
    } finally {
      if (valid(original)) setBusy(false);
    }
  }
  async function reveal(id: string) {
    if (!unlocked.current || busy) return;
    clearReveal();
    const original = epoch.current,
      sequence = revealSequence.current;
    try {
      const bytes = await port.vaultReveal(id);
      if (
        !valid(original) ||
        !unlocked.current ||
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
        setMessage(vaultError(error));
        clearLocal();
      }
    }
  }
  async function copy(id: string, field: "account" | "password") {
    if (!unlocked.current || busy) return;
    const original = epoch.current;
    try {
      await port.vaultCopy(id, field);
      if (valid(original) && unlocked.current) {
        setMessage(
          "已复制，剪贴板内容不会自动清除，将保留至你覆盖或手动清除。",
        );
        touch();
      }
    } catch (error) {
      if (valid(original)) {
        setMessage(vaultError(error));
        // Copy failures carry no trustworthy clipboard-only recovery proof.
        // Unknown transport errors are normalized to UNSUPPORTED by the bridge.
        clearLocal();
      }
    }
  }
  const filtered = rows.filter((r) =>
    `${r.channel}\n${r.account}`
      .toLocaleLowerCase()
      .includes(search.toLocaleLowerCase()),
  );
  return (
    <main className="vault-shell" onPointerDown={touch} onKeyDown={touch}>
      <header className="vault-header">
        <div className="vault-brand">
          <LockSimple size={25} />
          <span>
            拾序 <small>密码库</small>
          </span>
        </div>
        <span className="vault-state">
          {state === "unlocked" ? "已解锁" : "已锁定"}
        </span>
      </header>
      <div className="vault-body">
        {state !== "unlocked" ? (
          <section className="vault-locked">
            <div className="vault-emblem">
              <ShieldCheck size={42} />
            </div>
            <h1>{mode === "create" ? "新建密码库" : "密码库已锁定"}</h1>
            <p>主密码只锁密码库，日历与通知独立运行。</p>
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
                  disabled={busy}
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
                    disabled={busy}
                  />
                </label>
              )}
              <button className="primary" disabled={busy}>
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
                  disabled={busy}
                  onClick={() => {
                    setMode("unlock");
                    setMessage("");
                  }}
                >
                  返回解锁
                </button>
              ) : (
                <>
                  <button
                    disabled={busy}
                    onClick={() => {
                      masterForm.current?.reset();
                      setMode("create");
                      setMessage("");
                    }}
                  >
                    新建密码库
                  </button>
                  <button onClick={() => setForgot(!forgot)}>忘记主密码</button>
                </>
              )}
            </div>
            {forgot && (
              <p className="vault-help">
                主密码无法找回或绕过。请保留加密库和备份；没有主密码无法恢复其中的密码。
              </p>
            )}
            <p className="vault-availability">
              密码引擎尚未验证，暂不可解锁或保存。
            </p>
            {state === "unlocking" && (
              <button onClick={lock}>锁定密码库</button>
            )}
          </section>
        ) : (
          <>
            <div className="vault-toolbar">
              <div>
                <h1>密码库</h1>
                <p className="muted">显示 15 秒 · 无交互 5 分钟自动锁定</p>
              </div>
              <button onClick={lock}>锁定密码库</button>
            </div>
            <div className="vault-actions">
              <input
                aria-label="搜索渠道或账号"
                placeholder="搜索渠道或账号"
                value={search}
                onChange={(e) => setSearch(e.target.value)}
              />
              <button
                className="primary"
                disabled={busy}
                onClick={() => {
                  clearReveal();
                  setForm("new");
                }}
              >
                新增条目
              </button>
              <button
                disabled={busy}
                onClick={() => {
                  clearReveal();
                  setMode("change");
                }}
              >
                更改主密码
              </button>
            </div>
            {mode === "change" ? (
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
                  <button className="primary" disabled={busy}>
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
                  onSave={(value) =>
                    mutate(
                      form === "new"
                        ? { operation: "create", ...value }
                        : {
                            operation: "update",
                            id: form.entry_id,
                            expected_revision: form.revision,
                            ...value,
                          },
                    )
                  }
                />
              </section>
            ) : (
              <div className="vault-list">
                {filtered.map((row) => (
                  <article data-entry-id={row.entry_id} key={row.entry_id}>
                    <div className="vault-entry-heading">
                      <h2>{row.channel}</h2>
                      <button
                        data-edit
                        disabled={busy}
                        onClick={() => {
                          clearReveal();
                          setForm(row);
                        }}
                      >
                        编辑
                      </button>
                      <button disabled={busy} onClick={() => setDeletion(row)}>
                        删除
                      </button>
                    </div>
                    <div className="vault-value">
                      <span>账号</span>
                      <strong>{row.account}</strong>
                      <button
                        disabled={busy}
                        onClick={() => void copy(row.entry_id, "account")}
                      >
                        复制账号
                      </button>
                    </div>
                    <div className="vault-value">
                      <span>密码</span>
                      <code>
                        {visible === row.entry_id && secret.current
                          ? new TextDecoder().decode(secret.current)
                          : "••••••••"}
                      </code>
                      <button
                        disabled={busy}
                        onClick={() =>
                          visible === row.entry_id
                            ? clearReveal()
                            : void reveal(row.entry_id)
                        }
                      >
                        {visible === row.entry_id ? "隐藏密码" : "显示密码"}
                      </button>
                      <button
                        disabled={busy}
                        onClick={() => void copy(row.entry_id, "password")}
                      >
                        复制密码
                      </button>
                    </div>
                  </article>
                ))}
                {!filtered.length && (
                  <p className="vault-empty">
                    {search
                      ? "没有匹配的渠道或账号。"
                      : "暂无条目，新增一个账号开始整理。"}
                  </p>
                )}
              </div>
            )}
            {deletion && (
              <section
                role="dialog"
                aria-label="确认删除"
                className="vault-confirm"
              >
                <h2>确认删除</h2>
                <p>
                  删除「{deletion.channel}」的账号「{deletion.account}」？
                </p>
                <div className="vault-actions">
                  <button
                    disabled={busy}
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
                  <button disabled={busy} onClick={() => setDeletion(null)}>
                    取消
                  </button>
                </div>
              </section>
            )}
          </>
        )}
        <p role="status" className="vault-message">
          {message}
        </p>
      </div>
      <footer className="vault-footer">
        <LockSimple size={16} />
        密码留在独立窗口 · 日历与通知继续运行
      </footer>
    </main>
  );
}
