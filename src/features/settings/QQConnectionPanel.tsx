import { useEffect, useRef, useState } from "react";
import {
  BridgeError,
  mainBridge,
  type QQConnectionMetadata,
} from "../../contracts/bridge";
import { errorText } from "../calendar/EventDetails";
const connectionError = (e: unknown) =>
  e instanceof BridgeError
    ? e.code === "CONFLICT"
      ? "连接或配置冲突；请先断开活动来源，再刷新并重新连接。"
      : e.code === "STORAGE_FULL"
        ? "存储空间不足，连接配置未保存。"
        : e.code === "AUTH_FAILED"
          ? "凭据或当前用户保护验证失败；请检查本机配置。"
          : e.code === "DISCONNECTED"
            ? "连接未建立或已经断开；请刷新后重新连接。"
            : errorText(e)
    : errorText(e);
export function QQConnectionPanel({
  sourceId,
  port = mainBridge,
}: {
  sourceId: string;
  port?: typeof mainBridge;
}) {
  const input = useRef<HTMLInputElement>(null);
  const owned = useRef(
    new Set<{ token: Uint8Array; abort: AbortController }>(),
  );
  const [metadata, setMetadata] = useState<QQConnectionMetadata | null>(null);
  const [busy, setBusy] = useState(false),
    [message, setMessage] = useState("");
  useEffect(() => {
    let live = true;
    const field = input.current;
    port
      .qqConnectionRead(sourceId)
      .then((v) => {
        if (live) setMetadata(v);
      })
      .catch((e) => {
        if (live) setMessage(connectionError(e));
      });
    return () => {
      live = false;
      if (field) field.value = "";
      for (const request of owned.current) {
        request.token.fill(0);
        request.abort.abort();
      }
      owned.current.clear();
    };
  }, [sourceId, port]);
  async function action(run: () => Promise<unknown>) {
    setBusy(true);
    setMessage("");
    try {
      await run();
      setMetadata(await port.qqConnectionRead(sourceId));
    } catch (e) {
      setMessage(connectionError(e));
    } finally {
      if (input.current) input.current.value = "";
      setBusy(false);
    }
  }
  return (
    <section aria-label="QQ 连接配置">
      <h3>OneBot 11 纯文字连接</h3>
      <p>
        先保存获准账号与群白名单，再保存连接配置，最后点击连接。获准群的新文字通知自动入历，无需逐条确认。
      </p>
      <p role="status">
        {metadata?.active ? "本机接收连接已建立（真实 QQ 尚未验收）" : "未连接"}{" "}
        · {metadata?.has_credential ? "凭据已保存" : "尚无可用凭据"}
      </p>
      {metadata?.last_error && (
        <p role="status">
          上次连接控制：{connectionError(new BridgeError(metadata.last_error))}
        </p>
      )}
      <form
        onSubmit={(e) => {
          e.preventDefault();
          const endpoint = (
            e.currentTarget.elements.namedItem("endpoint") as HTMLInputElement
          ).value;
          const token = new TextEncoder().encode(input.current?.value ?? "");
          if (input.current) input.current.value = "";
          const request = { token, abort: new AbortController() };
          owned.current.add(request);
          void action(async () => {
            try {
              await port.qqConnectionSave(
                sourceId,
                endpoint,
                token,
                request.abort.signal,
              );
            } finally {
              token.fill(0);
              owned.current.delete(request);
            }
          });
        }}
      >
        <label>
          本机地址（如 127.0.0.1:3001 或 [::1]:3001）
          <input
            key={metadata?.endpoint ?? ""}
            name="endpoint"
            required
            maxLength={128}
            defaultValue={metadata?.endpoint ?? ""}
            autoComplete="off"
          />
        </label>
        <label>
          连接令牌（只写入，不回显）
          <input
            ref={input}
            type="password"
            name="token"
            required
            maxLength={4096}
            autoComplete="off"
          />
        </label>
        <button disabled={busy || metadata?.active}>保存连接配置</button>
      </form>
      <button
        disabled={busy || !metadata?.has_credential || metadata?.active}
        onClick={() => void action(() => port.qqConnect(sourceId))}
      >
        连接此来源
      </button>
      <button
        disabled={busy || !metadata?.active}
        onClick={() => void action(() => port.qqDisconnect(sourceId))}
      >
        断开此来源
      </button>
      <button disabled={busy} onClick={() => void action(async () => {})}>
        刷新连接状态
      </button>
      <p>
        当前仅支持一个活动来源。切换账号请先断开；冲突不会替换已连接来源。配置变更后须重新连接。保存不连接，重启不自动连接或重连。
      </p>
      <p>
        令牌由当前 Windows
        用户独立保护，锁库和关闭主窗口继续接收；完整退出停止接收。输入及本界面拥有的字节数组会尽力清除，浏览器、IPC
        和 HTTP 库的临时副本无法保证全部抹除。
      </p>
      {message && <p role="status">{message}</p>}
    </section>
  );
}
