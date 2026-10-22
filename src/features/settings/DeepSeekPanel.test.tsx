// @vitest-environment jsdom
import { expect, it } from "vitest";
import { act } from "react";
import { DeepSeekPanel } from "./DeepSeekPanel";
import { port, render, settings } from "../../test/d4";
it("key_is_cleared_and_never_returned", async () => {
  let saved = "";
  const r = await render(
    <DeepSeekPanel
      port={port({
        modelSettingsRead: async () => ({
          configured: false,
          model: "deepseek-flash",
          last_error: null,
        }),
        modelKeySave: async (value) => {
          saved = value;
        },
      })}
      data={settings}
      changed={async () => {}}
    />,
  );
  const input = r.host.querySelector<HTMLInputElement>("input[type=password]")!;
  input.value = "synthetic-token";
  await r.click("保存密钥");
  expect(saved).toBe("synthetic-token");
  expect(input.value).toBe("");
  expect(r.host.textContent).not.toContain("synthetic-token");
  expect(r.host.textContent).toContain("重启后需重新启用");
  await r.close();
});

it.each([
  ["AUTH_FAILED", "密钥无效或未获授权"],
  ["DISCONNECTED", "网络或 DeepSeek 服务暂不可用"],
])("connection_failure_%s_does_not_report_success", async (code, message) => {
  let changes = 0;
  const r = await render(
    <DeepSeekPanel
      port={port({
        modelSettingsRead: async () => ({
          configured: true,
          model: "deepseek-flash",
          last_error: null,
        }),
        modelTestConnection: async () => {
          throw new Error(code);
        },
      })}
      data={settings}
      changed={async () => {
        changes++;
      }}
    />,
  );
  try {
    await r.click("测试连接");
    expect(r.host.textContent).toContain(message);
    expect(r.host.textContent).not.toContain("连接测试通过");
    expect(changes).toBe(0);
    expect(
      [...r.host.querySelectorAll("button")].find(
        (b) => b.textContent === "测试连接",
      )!.disabled,
    ).toBe(false);
  } finally {
    await r.close();
  }
});

it("pending_connection_blocks_duplicate_requests_and_key_deletion", async () => {
  let complete!: () => void;
  const pending = new Promise<void>((resolve) => {
    complete = resolve;
  });
  let calls = 0,
    deletes = 0;
  const r = await render(
    <DeepSeekPanel
      port={port({
        modelSettingsRead: async () => ({
          configured: true,
          model: "deepseek-flash",
          last_error: null,
        }),
        modelTestConnection: async () => {
          calls++;
          await pending;
        },
        modelKeyDelete: async () => {
          deletes++;
        },
      })}
      data={settings}
      changed={async () => {}}
    />,
  );
  try {
    await r.click("测试连接");
    await r.click("测试连接");
    await r.click("删除密钥");
    expect(calls).toBe(1);
    expect(deletes).toBe(0);
    expect(
      r.host.querySelector<HTMLInputElement>("input[type=password]")!.disabled,
    ).toBe(true);
    await act(async () => {
      complete();
      await pending;
    });
    expect(r.host.textContent).toContain("连接测试通过（仅使用虚构文字）");
    expect(
      r.host.querySelector<HTMLInputElement>("input[type=password]")!.disabled,
    ).toBe(false);
  } finally {
    complete();
    await r.close();
  }
});
