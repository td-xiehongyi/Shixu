import { useEffect, useRef, useState } from "react";
import type { VaultSummary } from "../../contracts/domain";
import { vaultError } from "./errors";
import { guardForm, newline, newlineMessage } from "./inputGuard";
export interface EntryFormProps {
  initial?: VaultSummary;
  onSave: (value: {
    channel: string;
    account: string;
    description: string;
    password?: Uint8Array;
  }) => Promise<void>;
  onCancel?: () => void;
}
export function EntryForm({ initial, onSave, onCancel }: EntryFormProps) {
  const form = useRef<HTMLFormElement>(null);
  const mountedAccount = useRef<string | null>(null);
  const mountedChannel = useRef<string | null>(null);
  const mountedDescription = useRef<string | null>(null);
  const owned = useRef<Uint8Array | null>(null);
  const alive = useRef(true);
  const [busy, setBusy] = useState(false);
  const [changePassword, setChangePassword] = useState(false);
  const [error, setError] = useState("");
  useEffect(() => {
    alive.current = true;
    const element = form.current;
    const descriptionElement = element?.elements.namedItem(
      "description",
    ) as HTMLTextAreaElement | null;
    if (descriptionElement)
      descriptionElement.defaultValue = initial?.description ?? "";
    // StrictMode replays cleanup/setup on the same DOM; restore its defaults
    // before capturing the browser-normalized baseline for exact channel edits.
    element?.reset();
    mountedChannel.current =
      (element?.elements.namedItem("channel") as HTMLTextAreaElement | null)
        ?.value ?? null;
    mountedAccount.current =
      (element?.elements.namedItem("account") as HTMLTextAreaElement | null)
        ?.value ?? null;
    mountedDescription.current =
      (element?.elements.namedItem("description") as HTMLTextAreaElement | null)
        ?.value ?? null;
    const removeGuard = element
      ? guardForm(element, ["password"], setError)
      : () => {};
    return () => {
      removeGuard();
      alive.current = false;
      owned.current?.fill(0);
      element?.reset();
      if (descriptionElement) descriptionElement.defaultValue = "";
      for (const input of element?.querySelectorAll<
        HTMLInputElement | HTMLTextAreaElement
      >("input, textarea") ?? [])
        input.value = "";
      mountedChannel.current = null;
      mountedAccount.current = null;
      mountedDescription.current = null;
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
    const accountInput = element.elements.namedItem(
      "account",
    ) as HTMLTextAreaElement;
    const account =
      initial && accountInput.value === mountedAccount.current
        ? initial.account
        : accountInput.value;
    const input = element.elements.namedItem(
      "password",
    ) as HTMLInputElement | null;
    const changingPassword = !initial || changePassword;
    const descriptionInput = element.elements.namedItem(
      "description",
    ) as HTMLTextAreaElement;
    const description =
      initial && descriptionInput.value === mountedDescription.current
        ? (initial.description ?? "")
        : descriptionInput.value;
    if (input && newline.test(input.value)) {
      setError(newlineMessage);
      element.reset();
      (element.elements.namedItem("account") as HTMLTextAreaElement).value = "";
      input.value = "";
      return;
    }
    if (
      !channel.trim() ||
      !account.trim() ||
      (changingPassword && !input?.value)
    ) {
      setError(
        changingPassword ? "请填写渠道、账号和密码。" : "请填写渠道和账号。",
      );
      return;
    }
    const password = changingPassword
      ? new TextEncoder().encode(input!.value)
      : undefined;
    if (input) input.value = "";
    owned.current = password ?? null;
    setBusy(true);
    setError("");
    try {
      await onSave({
        channel,
        account,
        description,
        ...(password ? { password } : {}),
      });
    } catch (error) {
      if (alive.current) setError(vaultError(error));
    } finally {
      password?.fill(0);
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
        <textarea
          aria-label="账号"
          name="account"
          defaultValue={initial?.account ?? ""}
          maxLength={4096}
          required
          disabled={busy}
        />
      </label>
      {initial && (
        <button
          type="button"
          disabled={busy}
          onClick={() => {
            const input = form.current?.elements.namedItem(
              "password",
            ) as HTMLInputElement | null;
            if (input) input.value = "";
            owned.current?.fill(0);
            setChangePassword(!changePassword);
            setError("");
          }}
        >
          {changePassword ? "取消修改密码" : "修改密码"}
        </button>
      )}
      {(!initial || changePassword) && (
        <label>
          {initial ? "新密码" : "密码"}
          <input
            aria-label="密码"
            name="password"
            type="password"
            autoComplete="off"
            required
            disabled={busy}
          />
        </label>
      )}
      <label>
        描述
        <textarea
          aria-label="描述"
          name="description"
          defaultValue={initial?.description ?? ""}
          maxLength={4096}
          rows={3}
          disabled={busy}
        />
      </label>
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
