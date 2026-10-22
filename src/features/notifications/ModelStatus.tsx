import { useEffect, useState } from "react";
import { mainBridge } from "../../contracts/bridge";
import type { ModelProcessingStatus } from "../../contracts/domain";
import { errorText } from "../calendar/EventDetails";
import { ModelReview } from "./ModelReview";
const labels = {
  not_scheduled: "",
  queued: "等待模型分析",
  running: "模型分析中",
  applied: "模型结果已入历",
  pending: "模型结果待确认",
  non_event: "模型判断：非事项",
  failed: "模型分析失败",
};
const reasons: Record<string, string> = {
  AUTH_FAILED: "密钥无效，请检查设置",
  NETWORK_ERROR: "网络或服务暂不可用",
  INVALID_OUTPUT: "返回结果未通过校验",
  TEXT_TOO_LONG: "文字超过分析长度限制",
  QUEUE_FULL: "分析队列已满",
  SESSION_ENDED: "授权会话已结束，不会补传旧消息",
  STALE_SOURCE: "来源或消息已变更",
};
export function ModelStatus({
  messageKey,
  revoked,
  port = mainBridge,
}: {
  messageKey: string;
  revoked: boolean;
  port?: typeof mainBridge;
}) {
  const [status, setStatus] = useState<ModelProcessingStatus | null>(null),
    [error, setError] = useState(""),
    [busy, setBusy] = useState(false);
  useEffect(() => {
    let active = true;
    let pending = false;
    async function refresh() {
      if (pending) return;
      pending = true;
      try {
        const s = await port.modelStatus(messageKey);
        if (active) setStatus(s);
      } catch (e) {
        if (active) setError(errorText(e));
      } finally {
        pending = false;
      }
    }
    void refresh();
    const timer = setInterval(() => void refresh(), 2000);
    return () => {
      active = false;
      clearInterval(timer);
    };
  }, [port, messageKey]);
  if (revoked) return null;
  return (
    <div className="model-status">
      {status && status.state !== "not_scheduled" && (
        <span>
          {labels[status.state]}
          {status.reason ? ` · ${reasons[status.reason] ?? status.reason}` : ""}
        </span>
      )}
      {status?.state === "failed" && (
        <button
          disabled={busy}
          onClick={async () => {
            setBusy(true);
            try {
              await port.modelRetry(messageKey);
              setStatus(await port.modelStatus(messageKey));
              setError("");
            } catch (e) {
              setError(errorText(e));
            } finally {
              setBusy(false);
            }
          }}
        >
          重试分析
        </button>
      )}
      {status?.state === "pending" && (
        <ModelReview
          messageKey={messageKey}
          port={port}
          onChanged={() => {
            void port
              .modelStatus(messageKey)
              .then(setStatus)
              .catch((e) => setError(errorText(e)));
          }}
        />
      )}
      {error && <small role="status">{error}</small>}
    </div>
  );
}
