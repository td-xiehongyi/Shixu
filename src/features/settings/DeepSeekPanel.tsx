import { useEffect, useRef, useState } from "react";
import { mainBridge } from "../../contracts/bridge";
import type { SettingsSnapshot, ModelSettings } from "../../contracts/domain";
import { errorText } from "../calendar/EventDetails";
const modelError = (e: unknown) =>
  e instanceof Error && e.message === "AUTH_FAILED"
    ? "密钥无效或未获授权，请检查 API Key。"
    : e instanceof Error && e.message === "DISCONNECTED"
      ? "网络或 DeepSeek 服务暂不可用，请稍后重试。"
      : errorText(e);
export function DeepSeekPanel({
  port = mainBridge,
  data,
  changed,
}: {
  port?: typeof mainBridge;
  data: SettingsSnapshot;
  changed: () => Promise<void>;
}) {
  const [info, setInfo] = useState<ModelSettings | null>(null),
    [busy, setBusy] = useState(false),
    [message, setMessage] = useState("");
  const key = useRef<HTMLInputElement>(null);
  useEffect(() => {
    let active = true;
    port
      .modelSettingsRead()
      .then((v) => {
        if (active) setInfo(v);
      })
      .catch((e) => {
        if (active) setMessage(modelError(e));
      });
    return () => {
      active = false;
    };
  }, [port]);
  async function perform(action: () => Promise<void>, success: string) {
    setBusy(true);
    setMessage("");
    try {
      await action();
      setInfo(await port.modelSettingsRead());
      await changed();
      setMessage(success);
    } catch (e) {
      setMessage(modelError(e));
    } finally {
      setBusy(false);
    }
  }
  const groups = [
    ...new Set(
      data.sources
        .filter((s) => s.config.enabled)
        .flatMap((s) => s.config.allowed_group_ids),
    ),
  ];
  return (
    <section>
      <h2>DeepSeek 通知分析</h2>
      <p>
        指定群全部新文字会发给 DeepSeek，并产生 API
        调用费用；重启后需重新启用。历史聊天、附件和密码库内容不会上传。
      </p>
      <p>
        {info?.configured ? "密钥已配置" : "密钥未配置"} ·{" "}
        {info?.model ?? "deepseek-flash"}
      </p>
      {info?.last_error && (
        <p>最近连接测试：{modelError(new Error(info.last_error))}</p>
      )}
      <label>
        API Key
        <input
          ref={key}
          type="password"
          autoComplete="new-password"
          maxLength={8192}
          placeholder="仅在本机填写"
          disabled={busy}
        />
      </label>
      <button
        disabled={busy}
        onClick={() =>
          void perform(async () => {
            await port.modelKeySave(key.current?.value ?? "");
            if (key.current) key.current.value = "";
          }, "密钥已保存")
        }
      >
        保存密钥
      </button>
      <button
        disabled={busy || !info?.configured}
        onClick={() =>
          void perform(
            () => port.modelTestConnection(),
            "连接测试通过（仅使用虚构文字）",
          )
        }
      >
        测试连接
      </button>
      <button
        disabled={busy || !info?.configured}
        onClick={() =>
          void perform(() => port.modelKeyDelete(), "密钥已删除，分析已关闭")
        }
      >
        删除密钥
      </button>
      <form
        key={data.model.revision}
        onSubmit={(e) => {
          e.preventDefault();
          const form = new FormData(e.currentTarget);
          void perform(
            () =>
              port.setModelConsent({
                enabled: form.has("enabled"),
                provider_id: "deepseek",
                allowed_group_ids: form.getAll("group").map(String),
                allow_attachment_text: false,
                revision: data.model.revision,
              }),
            "分析设置已保存",
          );
        }}
      >
        <label>
          <input
            name="enabled"
            type="checkbox"
            defaultChecked={data.model.enabled}
            disabled={busy || !info?.configured}
          />
          启用通知分析
        </label>
        <fieldset disabled={busy}>
          <legend>允许分析的群</legend>
          {groups.length === 0 ? (
            <p>请先配置 QQ 接收群。</p>
          ) : (
            groups.map((g) => (
              <label key={g}>
                <input
                  type="checkbox"
                  name="group"
                  value={g}
                  defaultChecked={data.model.allowed_group_ids.includes(g)}
                />
                {g}
              </label>
            ))
          )}
        </fieldset>
        <button disabled={busy || !info}>保存分析设置</button>
      </form>
      {busy && (
        <p role="status">
          正在处理；关闭分析会等待已发出的请求结束（最多约30秒）。
        </p>
      )}
      {message && <p role="status">{message}</p>}
    </section>
  );
}
