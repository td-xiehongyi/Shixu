// @vitest-environment jsdom
import { expect, it } from "vitest";
import { ModelStatus } from "./ModelStatus";
import { port, render, id, event } from "../../test/d4";
it("distinguishes_non_event_failure_and_safe_retry", async () => {
  for (const state of ["non_event", "failed"] as const) {
    let retries = 0;
    const r = await render(
      <ModelStatus
        messageKey={id}
        revoked={false}
        port={port({
          modelStatus: async () => ({
            message_key: id,
            revision: "1",
            state,
            reason: state === "failed" ? "NETWORK_ERROR" : null,
            attempts: 1,
          }),
          modelRetry: async () => {
            retries++;
          },
        })}
      />,
    );
    expect(r.host.textContent).toContain(
      state === "failed" ? "网络或服务暂不可用" : "非事项",
    );
    if (state === "failed") {
      await r.click("重试分析");
      expect(retries).toBe(1);
    } else {
      expect(r.host.querySelector("button")).toBeNull();
    }
    await r.close();
  }
});

it("ignoring_pending_create_needs_no_date_or_target_and_keeps_review_token", async () => {
  let args: unknown[] = [];
  const r = await render(
    <ModelStatus
      messageKey={id}
      revoked={false}
      port={port({
        modelStatus: async () => ({
          message_key: id,
          revision: "1",
          state: "pending",
          reason: null,
          attempts: 1,
        }),
        modelReviews: async () =>
          args.length
            ? []
            : [
                {
                  candidate_key: id,
                  review_token: "b".repeat(64),
                  action: "create",
                  title: "班会",
                  time_text: "之后",
                  location: null,
                  evidence: "之后开班会",
                  reason: "UNCERTAIN_TIME_OR_ASSERTION",
                  targets: [],
                },
              ],
        modelReviewResolve: async (...values) => {
          args = values;
        },
      })}
    />,
  );
  try {
    await r.click("忽略此提取");
    expect(args).toEqual([id, "b".repeat(64), null, "0", null, true]);
    expect(r.host.querySelector("form")).toBeNull();
  } finally {
    await r.close();
  }
});

it("revoked_message_exposes_no_retry_or_confirmation_action", async () => {
  let reviews = 0;
  const r = await render(
    <ModelStatus
      messageKey={id}
      revoked={true}
      port={port({
        modelStatus: async () => ({
          message_key: id,
          revision: "1",
          state: "pending",
          reason: null,
          attempts: 1,
        }),
        modelReviews: async () => {
          reviews++;
          return [];
        },
      })}
    />,
  );
  try {
    expect(r.host.textContent).toBe("");
    expect(r.host.querySelector("button")).toBeNull();
    expect(reviews).toBe(0);
  } finally {
    await r.close();
  }
});
it("pending_cancel_shows_evidence_and_requires_a_chosen_target", async () => {
  let resolved = false;
  let args: unknown[] = [];
  const r = await render(
    <ModelStatus
      messageKey={id}
      revoked={false}
      port={port({
        modelStatus: async () => ({
          message_key: id,
          revision: "1",
          state: resolved ? "applied" : "pending",
          reason: null,
          attempts: 1,
        }),
        modelReviews: async () =>
          resolved
            ? []
            : [
                {
                  candidate_key: id,
                  review_token: "a".repeat(64),
                  action: "cancel",
                  title: "班会",
                  time_text: "取消班会",
                  location: null,
                  evidence: "取消班会",
                  reason: "AMBIGUOUS_TARGET_OR_ASSERTION",
                  targets: [event],
                },
              ],
        modelReviewResolve: async (...values) => {
          args = values;
          resolved = true;
        },
      })}
    />,
  );
  expect(r.host.textContent).toContain("无法唯一确认");
  expect(r.host.querySelector("blockquote")?.textContent).toBe("取消班会");
  await r.click("确认取消");
  expect(resolved).toBe(false);
  r.host.querySelector("select")!.value = id;
  await r.click("确认取消");
  expect(args).toEqual([id, "a".repeat(64), id, "1", null, false]);
  expect(r.host.textContent).not.toContain("模型待确认事项");
  await r.close();
});
