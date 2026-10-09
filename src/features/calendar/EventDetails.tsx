import { useEffect, useState } from "react";
import { mainBridge } from "../../contracts/bridge";
import type {
  CalendarDetails,
  CalendarEvent,
  EventPatch,
  Precision,
} from "../../contracts/domain";
import { SourceEvidence } from "../../components/SourceEvidence";
export const errorText = (e: unknown) =>
  e instanceof Error && e.message === "CONFLICT"
    ? "内容已更新，请刷新后重试。"
    : e instanceof Error && e.message === "INVALID_INPUT"
      ? "请检查日期、时区和字段格式。"
      : "此功能暂不可用，请在桌面应用中重试。";
export function EventDetails({
  event,
  port = mainBridge,
  onClose,
  onChanged,
}: {
  event: CalendarEvent | null;
  port?: typeof mainBridge;
  onClose: () => void;
  onChanged: () => void;
}) {
  const [detail, setDetail] = useState<CalendarDetails | null>(null),
    [error, setError] = useState(""),
    [busy, setBusy] = useState(false);
  useEffect(() => {
    let active = true;
    if (event)
      port
        .calendarDetails(event.event_id)
        .then((v) => {
          if (active) setDetail(v);
        })
        .catch((e) => {
          if (active) setError(errorText(e));
        });
    return () => {
      active = false;
    };
  }, [event, port]);
  async function save(form: HTMLFormElement) {
    setBusy(true);
    setError("");
    try {
      const d = new FormData(form);
      const precision = d.get("precision") as Precision;
      const date = String(d.get("date") || "");
      const start = String(d.get("start") || "");
      const end = String(d.get("end") || "");
      if (
        precision === "exact" &&
        (!/(Z|[+-]\d{2}:\d{2})$/.test(start) ||
          (end && !/(Z|[+-]\d{2}:\d{2})$/.test(end)))
      )
        throw new Error("INVALID_INPUT");
      const patch: EventPatch = {
        event_id: event?.event_id ?? crypto.randomUUID(),
        expected_revision: event?.revision ?? "0",
        title: String(d.get("title")),
        location: d.get("location")
          ? { operation: "set", value: String(d.get("location")) }
          : { operation: "clear" },
        status: d.get("status") as CalendarEvent["status"],
        time: {
          precision,
          local_date: precision === "unknown_date" ? null : date,
          start_at: precision === "exact" ? Date.parse(start) : null,
          end_at: precision === "exact" && end ? Date.parse(end) : null,
          timezone: String(d.get("timezone")),
          raw_time_text: precision === "exact" ? start : date,
        },
      };
      if (event) {
        if (patch.title === event.title) patch.title = null;
        const nextLocation =
          patch.location?.operation === "set" ? patch.location.value : null;
        if (nextLocation === event.location) patch.location = null;
        if (patch.status === event.status) patch.status = null;
        const t = patch.time!;
        if (
          t.precision === event.time_precision &&
          t.local_date === event.local_date &&
          t.start_at === event.start_at &&
          t.end_at === event.end_at &&
          t.timezone === event.timezone
        )
          patch.time = null;
      }
      if (event) await port.calendarEdit(event.event_id, event.revision, patch);
      else await port.createManualEvent(patch);
      onChanged();
      onClose();
    } catch (e) {
      setError(errorText(e));
    } finally {
      setBusy(false);
    }
  }
  return (
    <div className="dialog-backdrop">
      <section
        className="event-dialog"
        role="dialog"
        aria-modal="true"
        aria-label={event ? "日程详情" : "新建日程"}
      >
        <div className="title-row">
          <h2>{event ? "日程详情" : "新建日程"}</h2>
          <button onClick={onClose}>关闭</button>
        </div>
        <form
          onSubmit={(e) => {
            e.preventDefault();
            void save(e.currentTarget);
          }}
        >
          <label>
            标题
            <input
              name="title"
              required
              maxLength={500}
              defaultValue={event?.title}
            />
          </label>
          <label>
            时间类型
            <select
              name="precision"
              defaultValue={event?.time_precision ?? "date_only"}
            >
              <option value="date_only">已知日期 · 时间待定</option>
              <option value="unknown_date">日期待定</option>
              <option value="exact">明确时间</option>
              <option value="explicit_all_day">明确全天</option>
            </select>
          </label>
          <label>
            日期
            <input
              name="date"
              type="date"
              defaultValue={event?.local_date ?? ""}
            />
          </label>
          <label>
            开始时间（ISO，含时区偏移）
            <input
              name="start"
              placeholder="2026-10-09T09:00:00+08:00"
              defaultValue={
                event?.start_at != null
                  ? new Date(event.start_at).toISOString()
                  : ""
              }
            />
          </label>
          <label>
            结束时间（可留空）
            <input
              name="end"
              defaultValue={
                event?.end_at != null
                  ? new Date(event.end_at).toISOString()
                  : ""
              }
            />
          </label>
          <label>
            时区
            <input
              name="timezone"
              required
              defaultValue={
                event?.timezone ??
                Intl.DateTimeFormat().resolvedOptions().timeZone
              }
            />
          </label>
          <label>
            地点
            <input name="location" defaultValue={event?.location ?? ""} />
          </label>
          <label>
            状态
            <select name="status" defaultValue={event?.status ?? "active"}>
              <option value="active">有效</option>
              <option value="cancelled">已取消</option>
              <option value="removed">已移除</option>
            </select>
          </label>
          <button className="primary" disabled={busy}>
            保存日程
          </button>
        </form>
        {event && (
          <>
            <p>
              人工保护字段：{event.user_overrides.join("、") || "无"}；修订{" "}
              {event.revision}
            </p>
            <p>时间原文摘录：{event.raw_time_text}</p>
          </>
        )}
        {detail && (
          <>
            <p>
              来源：{detail.origin === "manual" ? "手动创建" : "通知自动入历"}
            </p>
            {detail.sources.map((s, i) => (
              <section key={`${s.message_key}-${i}`}>
                <p>
                  {s.group_id} ·{" "}
                  {s.outcome === "revoked"
                    ? "来源已撤回（不等于日程取消）"
                    : {
                        applied: "已入历",
                        pending: "待定",
                        conflict: "需要核对",
                        suppressed: "已抑制重复入历",
                        revoked: "来源已撤回",
                      }[s.outcome]}
                </p>
                <SourceEvidence blocks={s.evidence} />
              </section>
            ))}
            <h3>修改历史</h3>
            <p className="muted">
              最近 100 条变更。撤销校验当前修订，不覆盖更新内容。
            </p>
            {detail.history.map((h) => (
              <article key={h.change_id}>
                <span>
                  修订 {h.after.revision} · {h.after.title}{" "}
                  {h.undone ? "（已撤销）" : ""}
                </span>
                <button
                  disabled={
                    busy || h.undone || h.after.revision !== event?.revision
                  }
                  onClick={async () => {
                    setBusy(true);
                    try {
                      await port.calendarUndo({
                        change_id: h.change_id,
                        expected_revision: event!.revision,
                      });
                      onChanged();
                      onClose();
                    } catch (e) {
                      setError(errorText(e));
                    } finally {
                      setBusy(false);
                    }
                  }}
                >
                  撤销此变更
                </button>
              </article>
            ))}
          </>
        )}
        {error && <p role="alert">{error}</p>}
      </section>
    </div>
  );
}
