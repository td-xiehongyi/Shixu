// @vitest-environment jsdom
import { expect, it } from "vitest";
import { QQConnectionPanel } from "./QQConnectionPanel";
import { port, render, id } from "../../test/d4";
import { BridgeError } from "../../contracts/bridge";
for (const failed of [false, true]) {
  it(`clears_owned_token_and_dom_on_${failed ? "failure" : "success"}_without_claiming_connected`, async () => {
    let captured: Uint8Array | undefined;
    const r = await render(
      <QQConnectionPanel
        sourceId={id}
        port={port({
          qqConnectionRead: async () => ({
            endpoint: null,
            has_credential: false,
            active: false,
            last_error: null,
          }),
          qqConnectionSave: async (_id, _endpoint, token) => {
            captured = token;
            if (failed) throw new BridgeError("STORAGE_FULL");
          },
        })}
      />,
    );
    const input = r.host.querySelector<HTMLInputElement>(
      "input[type=password]",
    )!;
    input.value = "synthetic-token";
    r.host.querySelector<HTMLInputElement>("input[name=endpoint]")!.value =
      "127.0.0.1:12345";
    await r.click("保存连接配置");
    expect(input.value).toBe("");
    expect(captured?.every((b) => b === 0)).toBe(true);
    expect(r.host.textContent).toContain("未连接");
    input.value = "synthetic-token";
    await r.close();
    expect(input.value).toBe("");
  });
}
it("unmount_clears_owned_bytes_while_save_is_still_pending", async () => {
  let captured: Uint8Array | undefined;
  let complete!: () => void;
  const pending = new Promise<void>((resolve) => {
    complete = resolve;
  });
  const r = await render(
    <QQConnectionPanel
      sourceId={id}
      port={port({
        qqConnectionRead: async () => ({
          endpoint: null,
          has_credential: false,
          active: false,
          last_error: null,
        }),
        qqConnectionSave: async (_id, _endpoint, token) => {
          captured = token;
          await pending;
        },
      })}
    />,
  );
  r.host.querySelector<HTMLInputElement>("input[type=password]")!.value =
    "synthetic-token";
  r.host.querySelector<HTMLInputElement>("input[name=endpoint]")!.value =
    "127.0.0.1:12345";
  await r.click("保存连接配置");
  expect(captured?.some((b) => b !== 0)).toBe(true);
  await r.close();
  expect(captured?.every((b) => b === 0)).toBe(true);
  complete();
});
