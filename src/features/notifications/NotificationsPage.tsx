import { useEffect, useState } from "react";
import { Bell } from "@phosphor-icons/react/dist/csr/Bell";
import { Sparkle } from "@phosphor-icons/react/dist/csr/Sparkle";
import { CaretRight } from "@phosphor-icons/react/dist/csr/CaretRight";
import { UsersThree } from "@phosphor-icons/react/dist/csr/UsersThree";
import { mainBridge } from "../../contracts/bridge";
import type {
  MessageEnvelope,
  PartResult,
  PartStatus,
} from "../../contracts/domain";
import { SourceEvidence } from "../../components/SourceEvidence";
import { errorText } from "../calendar/EventDetails";
const labels: Record<PartStatus, string> = {
  pending_download: "等待下载",
  downloading: "下载中",
  fetched: "已下载",
  parsing: "解析中",
  success: "解析成功",
  download_failed: "无法下载",
  unsupported: "格式不支持",
  limit_exceeded: "超过限制",
  recognition_failed: "识别失败",
  partial_parse: "部分解析",
};
const reasons: Record<string, string> = {
  DOWNLOAD_UNAVAILABLE: "暂时无法下载",
  FORMAT_UNSUPPORTED: "格式不支持",
  LIMIT_EXCEEDED: "超过限制",
  RECOGNITION_FAILED: "识别失败",
  PARTIAL_SOURCE: "来源不完整",
  TIMED_OUT: "处理超时",
  MEMORY_LIMIT: "超出内存限制",
  AUTH_REQUIRED: "需要登录",
  EXPIRED: "来源已过期",
  PERMISSION_DENIED: "无访问权限",
  STORAGE_FULL: "存储空间不足",
};
const demoNotices = [
  {
    group: "示例班级群",
    stamp: "今天 08:10",
    text: "高数考试09:00，教学楼A201",
    state: "已自动加入日历",
  },
  {
    group: "示例英语群",
    stamp: "昨天 18:30",
    text: "周日14:00英语模拟考试",
    state: "已自动加入日历",
  },
  {
    group: "示例活动群",
    stamp: "昨天 16:20",
    text: "交流会时间另行通知",
    state: "时间待定",
  },
];
function Notice({
  message: m,
  port,
}: {
  message: MessageEnvelope;
  port: typeof mainBridge;
}) {
  const [parts, setParts] = useState<PartResult[]>([]),
    [open, setOpen] = useState(false),
    [error, setError] = useState(""),
    [busy, setBusy] = useState(false);
  useEffect(() => {
    let active = true;
    setParts([]);
    port
      .notificationParts(m.message_key)
      .then((p) => {
        if (active) setParts(p);
      })
      .catch((e) => {
        if (active) setError(errorText(e));
      });
    return () => {
      active = false;
    };
  }, [port, m.message_key, m.revision]);
  const failed = m.parts.filter(
    (p) =>
      p.kind === "image" &&
      (parts.find((r) => r.part_id === p.part_id)?.status ?? p.parse_state) ===
        "download_failed",
  ).length;
  return (
    <article className="notice">
      <div className="group-icon">
        <UsersThree size={23} weight="fill" />
      </div>
      <div className="notice-content">
        <div className="notice-heading">
          <h3>{m.group_id}</h3>
          <button className="source-link" onClick={() => setOpen(!open)}>
            来源原文 <CaretRight size={14} />
          </button>
        </div>
        <small>
          接收 {new Date(m.received_at).toLocaleString()} · 修订 {m.revision}
        </small>
        <p className="plain-text">{m.text}</p>
        <span className="notice-state">
          {m.revoked
            ? "来源已撤回（不等于日程取消）"
            : m.calendar_applied
              ? failed
                ? `正文已入历，另 ${failed} 张图片无法下载`
                : "已处理并提交日历"
              : {
                  committed: "处理已提交",
                  persisted: "已持久化，等待处理",
                  parsing: "处理中",
                  retryable_failure: "处理失败，可重试",
                  unparseable: "无法解析",
                  non_event: "非事项",
                  pending: "时间或关联待定",
                  source_revoked: "来源已撤回",
                }[m.processing_state]}
        </span>
        {m.parts.map((p) => {
          const result = parts.find((r) => r.part_id === p.part_id);
          const state = result?.status ?? p.parse_state;
          return (
            <div className="part-status" key={p.part_id}>
              <span>
                {p.original_name ?? (p.kind === "image" ? "图片" : "文件")} ·{" "}
                {labels[state]} ·{" "}
                {reasons[result?.reason_code ?? p.failure_code ?? ""] ?? ""}
              </span>
              <small>
                {p.fetch_state === "unavailable" || p.fetch_state === "pending"
                  ? "原件不可用"
                  : "原件受控预览尚未接通"}
              </small>
              {state === "download_failed" && !m.revoked && (
                <button
                  disabled={busy}
                  onClick={async () => {
                    setBusy(true);
                    try {
                      await port.retryPart(p.part_id);
                      setParts(await port.notificationParts(m.message_key));
                      setError("重试已提交；保留原日程与人工修改。");
                    } catch (e) {
                      setError(errorText(e));
                    } finally {
                      setBusy(false);
                    }
                  }}
                >
                  重试部件
                </button>
              )}
            </div>
          );
        })}
        {open && (
          <div>
            <p className="muted">
              原文为纯文本摘录，最多 4096 字节。完整证据保留在本机。
            </p>
            <pre>{m.text}</pre>
            <SourceEvidence blocks={parts.flatMap((p) => p.blocks)} />
          </div>
        )}
        {error && <p role="status">{error}</p>}
      </div>
    </article>
  );
}
export function NotificationsPage({
  port = mainBridge,
  demo = false,
  full = false,
}: {
  port?: typeof mainBridge;
  demo?: boolean;
  full?: boolean;
}) {
  const [messages, setMessages] = useState<MessageEnvelope[]>([]),
    [error, setError] = useState(""),
    [refresh, setRefresh] = useState(0);
  useEffect(() => {
    let active = true;
    setMessages([]);
    setError("");
    if (demo) return;
    port
      .notificationList()
      .then((m) => {
        if (active) setMessages(m);
      })
      .catch((e) => {
        if (active) setError(errorText(e));
      });
    return () => {
      active = false;
    };
  }, [port, demo, refresh]);
  return (
    <aside
      className={`notifications ${full ? "full" : ""}`}
      aria-label="QQ通知"
    >
      <h2>QQ通知</h2>
      <p className={`collect-state ${demo ? "demo-state" : ""}`}>
        <span className="status-dot" />
        {demo ? "已选群正在采集（模拟）" : "QQ 未连接"}
      </p>
      <p className="run-hint">
        {demo
          ? "仅电脑开机时运行 · 演示数据"
          : "接收、持久化、入历状态尚未接通验证"}
      </p>
      {!demo && (
        <button onClick={() => setRefresh((v) => v + 1)}>刷新通知</button>
      )}
      <div className="notice-list">
        {demo
          ? demoNotices.map((n) => (
              <article className="notice" key={n.group}>
                <div className="group-icon">
                  <UsersThree size={23} weight="fill" />
                </div>
                <div className="notice-content">
                  <div className="notice-heading">
                    <h3>{n.group}</h3>
                    <span className="source-link">模拟来源</span>
                  </div>
                  <div className="notice-meta">{n.stamp}</div>
                  <p>{n.text}</p>
                  <span
                    className={`notice-state ${n.state === "时间待定" ? "pending" : ""}`}
                  >
                    {n.state}
                  </span>
                </div>
              </article>
            ))
          : messages.map((m) => (
              <Notice
                key={`${m.message_key}-${m.revision}`}
                message={m}
                port={port}
              />
            ))}
      </div>
      {!demo && !messages.length && (
        <div className="empty-notice">
          <Bell size={28} />
          <p>暂无已保留通知。</p>
        </div>
      )}
      {error && <p role="status">{error}</p>}
      {!demo && (
        <p className="muted">
          显示最近 100 条保留通知，含已处理、失败与撤回状态。
        </p>
      )}
      <div className="future-agent" aria-disabled="true">
        <Sparkle size={23} />
        <span>个人智能体</span>
        <small>后续开放</small>
        <CaretRight size={16} />
      </div>
    </aside>
  );
}
