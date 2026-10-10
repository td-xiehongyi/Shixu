import { QQConnectionPanel } from "./QQConnectionPanel";
import { BackupPanel } from "./BackupPanel";
import { useEffect, useState } from "react";
import { mainBridge } from "../../contracts/bridge";
import type { SettingsSnapshot, SourceConfig } from "../../contracts/domain";
import { errorText } from "../calendar/EventDetails";
const initial: SettingsSnapshot = {
  sources: [],
  model: {
    enabled: false,
    provider_id: null,
    allowed_group_ids: [],
    allow_attachment_text: false,
    revision: "0",
  },
  autostart: false,
  transport_supported: false,
  runtime: {
    running: false,
    pending_rules: 0,
    attachment_queue: 0,
    model_queue: 0,
    last_calendar_commit: null,
    last_error: null,
    sources: [],
  },
};
export function SettingsPage({
  port = mainBridge,
  demo = false,
  onDemo,
}: {
  port?: typeof mainBridge;
  demo?: boolean;
  onDemo?: (on: boolean) => void;
}) {
  const [data, setData] = useState(initial),
    [ready, setReady] = useState(false),
    [error, setError] = useState(""),
    [busy, setBusy] = useState(false),
    [selected, setSelected] = useState("");
  useEffect(() => {
    let active = true;
    port
      .settingsRead()
      .then((v) => {
        if (active) {
          setData(v);
          setReady(true);
        }
      })
      .catch((e) => {
        if (active) setError(errorText(e));
      });
    return () => {
      active = false;
    };
  }, [port]);
  async function change(action: () => Promise<void>) {
    setBusy(true);
    setError("");
    try {
      await action();
      setData(await port.settingsRead());
      setError("设置已保存。");
    } catch (e) {
      setError(errorText(e));
    } finally {
      setBusy(false);
    }
  }
  const row = data.sources.find((s) => s.config.source_id === selected);
  const config = row?.config;
  return (
    <main className="module-content settings-page">
      <h1>设置</h1>
      <p>QQ 未连接前不会接收；连接、持久化、入历和后台运行须分别验证。</p>
      <p>模型服务尚未接通；保存授权不会发起网络请求。</p>
      {ready && (
        <section aria-label="后台状态">
          <h2>后台状态</h2>
          <p>
            {data.runtime.running ? "调度运行中" : "调度未启动"} · 待入历{" "}
            {data.runtime.pending_rules} · 附件 {data.runtime.attachment_queue}{" "}
            · 模型 {data.runtime.model_queue}
          </p>
          {data.runtime.last_error && (
            <p role="status">
              {data.runtime.last_error === "STORAGE_FULL"
                ? "存储空间不足；接收或入历尚未完成。"
                : "后台任务尚未完成，请检查来源连接与权限。"}
            </p>
          )}
          {data.runtime.sources.map((source) => (
            <p key={source.source_id}>
              来源 {source.source_id} ·{" "}
              {source.connection_state === "connected"
                ? "已接收在线消息"
                : "未连接"}{" "}
              · 接收 {source.last_received_at ?? "无"} · 持久化{" "}
              {source.last_persisted_at ?? "无"} · 入历{" "}
              {source.last_applied_at ?? "无"}
              {source.gap ? " · 存在未核实缺口" : ""}
            </p>
          ))}
          <button
            disabled={busy}
            onClick={() => {
              void change(async () => {});
            }}
          >
            刷新后台状态
          </button>
        </section>
      )}
      <section>
        <h2>通知来源</h2>
        <p className="muted">
          白名单与时区变更仅用于新通知；历史来源、原有消息身份和时间依据保留。每次保存记录新配置期次。
        </p>
        <label>
          来源
          <select
            value={selected}
            onChange={(e) => setSelected(e.target.value)}
          >
            <option value="">新建来源</option>
            {data.sources.map((s) => (
              <option key={s.config.source_id} value={s.config.source_id}>
                {s.config.account_id} · 配置期次 {s.epoch}
              </option>
            ))}
          </select>
        </label>
        <form
          key={selected + (row?.epoch ?? "")}
          onSubmit={(e) => {
            e.preventDefault();
            const form = new FormData(e.currentTarget);
            const c: SourceConfig = {
              source_id: config?.source_id ?? crypto.randomUUID(),
              adapter_type: String(form.get("adapter")),
              account_id: String(form.get("account")),
              allowed_group_ids: String(form.get("groups"))
                .split(/[,，\n]/)
                .map((s) => s.trim())
                .filter(Boolean),
              timezone: String(form.get("timezone")),
              enabled: form.has("enabled"),
              capability_set: config?.capability_set ?? ["live_messages"],
            };
            void change(async () => {
              await port.saveSourceConfig(c);
              setSelected(c.source_id);
            });
          }}
        >
          <label>
            接入类型
            <input
              name="adapter"
              required
              maxLength={128}
              readOnly={!!config}
              defaultValue={config?.adapter_type ?? "onebot11-text"}
            />
          </label>
          <label>
            账号标识
            <input
              name="account"
              required
              maxLength={128}
              readOnly={!!config}
              defaultValue={config?.account_id}
            />
          </label>
          <label>
            群白名单（逗号分隔）
            <textarea
              name="groups"
              defaultValue={config?.allowed_group_ids.join(",")}
            />
          </label>
          <label>
            来源时区
            <input
              name="timezone"
              required
              defaultValue={config?.timezone ?? "Asia/Shanghai"}
            />
          </label>
          <label>
            <input
              type="checkbox"
              name="enabled"
              defaultChecked={config?.enabled ?? false}
            />
            允许采集这些群
          </label>
          <button className="primary" disabled={!ready || busy}>
            保存来源
          </button>
        </form>
        {selected && (
          <QQConnectionPanel
            key={"qq-" + selected + (row?.epoch ?? "")}
            sourceId={selected}
            port={port}
          />
        )}
      </section>
      <section>
        <h2>云端模型授权</h2>
        <form
          key={data.model.revision}
          onSubmit={(e) => {
            e.preventDefault();
            const d = new FormData(e.currentTarget);
            void change(() =>
              port.setModelConsent({
                enabled: d.has("enabled"),
                provider_id: String(d.get("provider")) || null,
                allowed_group_ids: String(d.get("groups"))
                  .split(/[,，\n]/)
                  .map((s) => s.trim())
                  .filter(Boolean),
                allow_attachment_text: d.has("attachments"),
                revision: data.model.revision,
              }),
            );
          }}
        >
          <label>
            <input
              type="checkbox"
              name="enabled"
              defaultChecked={data.model.enabled}
            />
            启用云端模型授权
          </label>
          <label>
            提供方标识
            <input
              name="provider"
              maxLength={128}
              defaultValue={data.model.provider_id ?? ""}
            />
          </label>
          <label>
            允许上传文本的群
            <textarea
              name="groups"
              defaultValue={data.model.allowed_group_ids.join(",")}
            />
          </label>
          <label>
            <input
              type="checkbox"
              name="attachments"
              defaultChecked={data.model.allow_attachment_text}
            />
            另外允许附件提取文本（不含原件）
          </label>
          <button disabled={!ready || busy}>保存模型授权</button>
        </form>
        <p className="muted">
          仅当前允许的群和单独授权的附件文本可发送。授权撤回立即生效；重启后模型授权默认关闭。
        </p>
      </section>
      <section>
        <h2>启动与锁库</h2>
        <label>
          <input
            type="checkbox"
            checked={data.autostart}
            disabled={!ready || busy}
            onChange={(e) => {
              const on = e.target.checked;
              void change(() => port.setAutostart(on));
            }}
          />
          登录自启策略
        </label>
        <p className="muted">
          策略可保存；Windows 登录注册尚未接通。启动时保持锁库。
        </p>
        <p>
          5 分钟无密码库交互自动锁定；密码显示 15
          秒恢复掩码；复制内容不会自动清除，
          将保留至你覆盖或手动清除。锁库不暂停日历和通知。
        </p>
      </section>
      <BackupPanel
        port={port}
        onRestored={async () => {
          setData(await port.settingsRead());
        }}
      />
      <div className="demo-setting">
        <label>
          <input
            type="checkbox"
            checked={demo}
            onChange={(e) => onDemo?.(e.target.checked)}
          />
          显示演示数据
        </label>
        <p className="muted">示例群与日程均为虚构，不代表真实连接状态。</p>
      </div>
      {error && <p role="status">{error}</p>}
    </main>
  );
}
