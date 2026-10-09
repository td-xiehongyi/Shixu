import { useEffect, useRef, useState } from "react";
import { mainBridge } from "../../contracts/bridge";
import type { BackupPreview, BackupSummary } from "../../contracts/domain";
import { errorText } from "../calendar/EventDetails";
export function BackupPanel({
  port = mainBridge,
  onRestored,
}: {
  port?: typeof mainBridge;
  onRestored?: () => Promise<void>;
}) {
  const [items, setItems] = useState<BackupSummary[]>([]),
    [preview, setPreview] = useState<BackupPreview | null>(null),
    [busy, setBusy] = useState(false),
    [status, setStatus] = useState(""),
    [exporting, setExporting] = useState(false),
    [raw, setRaw] = useState(false);
  const alive = useRef(true);
  useEffect(() => {
    alive.current = true;
    port
      .backupList()
      .then((v) => {
        if (alive.current) setItems(v);
      })
      .catch(() => {});
    return () => {
      alive.current = false;
    };
  }, [port]);
  async function run(action: () => Promise<void>) {
    setBusy(true);
    setStatus("");
    try {
      await action();
    } catch (e) {
      if (alive.current) setStatus(errorText(e));
    } finally {
      if (alive.current) setBusy(false);
    }
  }
  async function refresh() {
    const next = await port.backupList();
    if (alive.current) setItems(next);
  }
  return (
    <section aria-label="备份与迁移">
      <h2>备份与迁移</h2>
      <p>
        日历自动快照每日最多一份，保留最近 7 份；另保留最近一次恢复前快照。DPAPI
        本地快照只限原 Windows 用户保护环境，不保证跨机恢复。
      </p>
      <p>
        密码库加密备份应保留最近 10
        份；当前真实引擎尚未接通。旧备份可能需要旧主密码。删除当前内容不等于抹除已有备份。
      </p>
      <button
        disabled={busy}
        onClick={() =>
          void run(async () => {
            await port.backupSnapshot();
            await refresh();
            if (alive.current) setStatus("快照已保存并验证。");
          })
        }
      >
        创建今日日历快照
      </button>
      <button
        disabled={busy}
        onClick={() =>
          void run(async () => {
            const p = await port.backupPrevious();
            if (alive.current) setPreview(p);
          })
        }
      >
        查看恢复前快照
      </button>
      <button
        disabled={busy}
        onClick={() =>
          void run(async () => {
            await port.backupVault();
          })
        }
      >
        密码库加密备份
      </button>
      <ul>
        {items.map((m) => (
          <li key={m.created_at}>
            {new Date(m.created_at).toLocaleDateString()} · {m.events} 项事项{" "}
            <button
              disabled={busy}
              onClick={() =>
                void run(async () => {
                  const p = await port.backupPreview(
                    Math.floor(m.created_at / 86400000),
                  );
                  if (alive.current) setPreview(p);
                })
              }
            >
              预览恢复
            </button>
            <button
              disabled={busy}
              onClick={() =>
                void run(async () => {
                  await port.backupDelete(Math.floor(m.created_at / 86400000));
                  await refresh();
                  if (alive.current) {
                    setPreview(null);
                    setStatus("所选快照已删除。");
                  }
                })
              }
            >
              删除此快照
            </button>
          </li>
        ))}
      </ul>
      <p>
        主动导出明文 JSON
        含日程、必要聊天摘录、来源标识和变更历史。默认不含完整原始聊天、附件二进制或接入凭据；被省略的正文待处理工作不能继续提取。请自行妥善保管导出文件。
      </p>
      <button disabled={busy} onClick={() => setExporting(true)}>
        导出日历 JSON
      </button>
      {exporting && (
        <div role="group" aria-label="确认明文导出">
          <label>
            <input
              type="checkbox"
              checked={raw}
              onChange={(e) => setRaw(e.target.checked)}
            />
            另外包含完整原始聊天（仍不含附件二进制或凭据）
          </label>
          <button
            disabled={busy}
            onClick={() =>
              void run(async () => {
                const data = await port.backupExport(raw, true);
                if (!alive.current) return;
                const url = URL.createObjectURL(
                  new Blob([data], { type: "application/json" }),
                );
                const a = document.createElement("a");
                a.href = url;
                a.download = "shixu-calendar.json";
                a.click();
                URL.revokeObjectURL(url);
                setExporting(false);
                setStatus("已生成明文 JSON 下载，请检查浏览器保存结果。");
              })
            }
          >
            确认导出明文
          </button>
          <button disabled={busy} onClick={() => setExporting(false)}>
            取消导出
          </button>
        </div>
      )}
      <label>
        导入日历 JSON（最多 20 MiB，仅准备预览）
        <input
          type="file"
          accept=".json,application/json"
          disabled={busy}
          onChange={(e) => {
            const file = e.target.files?.[0];
            e.target.value = "";
            if (file)
              void run(async () => {
                if (file.size > 20 * 1024 * 1024)
                  throw new Error("INVALID_INPUT");
                const p = await port.backupImport(await file.text());
                if (alive.current) setPreview(p);
              });
          }}
        />
      </label>
      {preview && (
        <div role="group" aria-label="确认恢复">
          <h3>恢复预览</h3>
          <p>
            {preview.events} 项事项 · {preview.messages} 条来源记录
          </p>
          <p>
            原件：已保留 {preview.present} · 从未取得 {preview.never_fetched} ·
            已清理 {preview.cleaned} · 未迁移 {preview.not_migrated}
          </p>
          <p>
            确认将覆盖当前日历并先保留恢复前快照；不自动合并。来源采集、模型和自启将关闭，需要重新配置授权。预览有效期
            10 分钟。
          </p>
          <button
            disabled={busy}
            onClick={() =>
              void run(async () => {
                await port.backupRestore(preview.preview_id, true);
                await onRestored?.();
                if (alive.current) {
                  setPreview(null);
                  setStatus("恢复已完成；来源与模型保持关闭。");
                }
                await refresh();
              })
            }
          >
            确认覆盖恢复
          </button>
          <button disabled={busy} onClick={() => setPreview(null)}>
            取消恢复
          </button>
        </div>
      )}
      {status && <p role="status">{status}</p>}
    </section>
  );
}
