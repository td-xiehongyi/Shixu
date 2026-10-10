import { useEffect, useRef, useState } from "react";
import type { VaultSummary } from "../../contracts/domain";
import { vaultError } from "./errors";
import { guardForm, newline, newlineMessage } from "./inputGuard";
export interface EntryFormProps {
  initial?: VaultSummary;
  onSave: (value: {
    channel: string;
    account: string;
    password: Uint8Array;
  }) => Promise<void>;
  onCancel?: () => void;
}
export function EntryForm({ initial, onSave, onCancel }: EntryFormProps) {
  const form = useRef<HTMLFormElement>(null);
  const mountedChannel = useRef<string | null>(null);
  const owned = useRef<Uint8Array | null>(null);
  const alive = useRef(true);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  useEffect(() => {
    alive.current = true;
    const element = form.current;
    // StrictMode replays cleanup/setup on the same DOM; restore its defaults
    // before capturing the browser-normalized baseline for exact channel edits.
    element?.reset();
    mountedChannel.current =
      (element?.elements.namedItem("channel") as HTMLTextAreaElement | null)
        ?.value ?? null;
    const removeGuard = element
      ? guardForm(element, ["account", "password"], setError)
      : () => {};
    return () => {
      removeGuard();
      alive.current = false;
      owned.current?.fill(0);
      element?.reset();
      for (const input of element?.querySelectorAll<
        HTMLInputElement | HTMLTextAreaElement
      >("input, textarea") ?? [])
        input.value = "";
      mountedChannel.current = null;
    };
  }, []);
  async function submit(e: React.FormEvent) {
    e.preventDefault();
    if (busy) return;
    const element = form.current!;
    const channelInput = element.elements.namedItem(
      "channel",
    ) as HTMLTextAreaElement;
    const channel =
      initial && channelInput.value === mountedChannel.current
        ? initial.channel
        : channelInput.value;
    const account = (element.elements.namedItem("account") as HTMLInputElement)
      .value;
    const input = element.elements.namedItem("password") as HTMLInputElement;
    if (newline.test(account) || newline.test(input.value)) {
      setError(newlineMessage);
      element.reset();
      (element.elements.namedItem("account") as HTMLInputElement).value = "";
      input.value = "";
      return;
    }
    if (!channel || !account || !input.value) {
      setError("请填写渠道、账号和密码。");
      return;
    }
    const password = new TextEncoder().encode(input.value);
    input.value = "";
    owned.current = password;
    setBusy(true);
    setError("");
    try {
      await onSave({ channel, account, password });
    } catch (error) {
      if (alive.current) setError(vaultError(error));
    } finally {
      password.fill(0);
      owned.current = null;
      if (alive.current) setBusy(false);
    }
  }
  return (
    <form ref={form} onSubmit={submit} className="vault-form">
      <label>
        渠道
        <textarea
          aria-label="渠道"
          name="channel"
          defaultValue={initial?.channel ?? ""}
          maxLength={4096}
          required
          disabled={busy}
        />
      </label>
      <label>
        账号
        <input
          aria-label="账号"
          name="account"
          defaultValue={initial?.account ?? ""}
          maxLength={4096}
          required
          disabled={busy}
        />
      </label>
      <label>
        密码
        <input
          aria-label="密码"
          name="password"
          type="password"
          autoComplete="off"
          required
          disabled={busy}
        />
      </label>
      <p className="muted">
        {initial ? "输入完整密码后保存。" : "仅保存渠道、账号和密码。"}
      </p>
      <p role="alert">{error}</p>
      <div className="vault-actions">
        <button className="primary" disabled={busy}>
          {busy ? "保存中…" : "保存条目"}
        </button>
        <button type="button" onClick={onCancel}>
          取消
        </button>
      </div>
    </form>
  );
}
