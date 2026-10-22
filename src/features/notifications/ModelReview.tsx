import { useEffect, useState } from "react";
import { mainBridge } from "../../contracts/bridge";
import type { ModelReview as Review } from "../../contracts/domain";
import { errorText } from "../calendar/EventDetails";
const reasons: Record<string, string> = {
  USER_CONFLICT: "与已有人工修改冲突",
  UNCERTAIN_TIME_OR_ASSERTION: "时间或事项陈述不够明确",
  AMBIGUOUS_TARGET_OR_ASSERTION: "无法唯一确认事项或关联的日程",
};
export function ModelReview({
  messageKey,
  port,
  onChanged,
}: {
  messageKey: string;
  port: typeof mainBridge;
  onChanged: () => void;
}) {
  const [items, setItems] = useState<Review[]>([]),
    [error, setError] = useState(""),
    [busy, setBusy] = useState(false);
  useEffect(() => {
    let active = true;
    port
      .modelReviews(messageKey)
      .then((v) => {
        if (active) setItems(v);
      })
      .catch((e) => {
        if (active) setError(errorText(e));
      });
    return () => {
      active = false;
    };
  }, [messageKey, port]);
  async function resolve(
    r: Review,
    form: HTMLFormElement | null,
    dismiss: boolean,
  ) {
    setBusy(true);
    setError("");
    try {
      const d = form ? new FormData(form) : null;
      const id = String(d?.get("target") ?? "");
      const target = r.targets.find((e) => e.event_id === id);
      if (!dismiss && r.action !== "create" && !target)
        throw new Error("INVALID_INPUT");
      await port.modelReviewResolve(
        r.candidate_key,
        r.review_token,
        target?.event_id ?? null,
        target?.revision ?? "0",
        dismiss || r.action === "cancel" ? null : String(d?.get("time") ?? ""),
        dismiss,
      );
      setItems(await port.modelReviews(messageKey));
      onChanged();
    } catch (e) {
      setError(errorText(e));
    } finally {
      setBusy(false);
    }
  }
  return (
    <section aria-label="模型待确认事项">
      {items.map((r) => (
        <form
          key={r.candidate_key}
          onSubmit={(e) => {
            e.preventDefault();
            void resolve(r, e.currentTarget, false);
          }}
        >
          <h4>
            {{ create: "新增", reschedule: "改期", cancel: "取消" }[r.action]}：
            {r.title}
          </h4>
          <p>{reasons[r.reason] ?? r.reason}</p>
          <p>
            时间原文：{r.time_text || "未提取"} · 地点：{r.location ?? "未提取"}
          </p>
          <blockquote>{r.evidence}</blockquote>
          <label>
            关联日程
            <select
              name="target"
              disabled={busy}
              defaultValue={
                r.action === "create" &&
                r.targets.length === 1 &&
                r.targets[0].time_precision === "unknown_date"
                  ? r.targets[0].event_id
                  : ""
              }
              required={r.action !== "create"}
            >
              <option value="">
                {r.action === "create"
                  ? "新建日程（请先检查是否重复）"
                  : "请选择要更改的日程"}
              </option>
              {r.targets.map((e) => (
                <option value={e.event_id} key={e.event_id}>
                  {e.local_date ?? "日期待定"} · {e.title}
                </option>
              ))}
            </select>
          </label>
          {r.action !== "cancel" && (
            <label>
              确认后的日期和时间
              <input
                name="time"
                required
                maxLength={512}
                placeholder="例如 2026年10月15日14:00"
                disabled={busy}
              />
            </label>
          )}
          <button disabled={busy} type="submit">
            确认
            {
              { create: "入历", reschedule: "改期", cancel: "取消日程" }[
                r.action
              ]
            }
          </button>
          <button
            disabled={busy}
            type="button"
            onClick={() => void resolve(r, null, true)}
          >
            忽略此提取
          </button>
        </form>
      ))}
      {error && <p role="status">{error}</p>}
    </section>
  );
}
