const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const { chromium } = require(
  process.env.SHIXU_PLAYWRIGHT_MODULE || "playwright",
);
(async () => {
  const browser = await chromium.launch({
    executablePath: process.env.SHIXU_CHROMIUM || "/usr/bin/chromium",
    headless: true,
  });
  const page = await browser.newPage({ viewport: { width: 640, height: 780 } });
  const errors = [];
  page.on("pageerror", (e) => errors.push(e.message));
  page.on("console", (m) => {
    if (m.type() === "error") errors.push(m.text());
  });
  const out =
    process.env.SHIXU_BROWSER_EVIDENCE_DIR || ".superpowers/sdd/shixu-v0.1";
  fs.mkdirSync(out, { recursive: true });
  const captures = [];
  async function capture(name) {
    await page.screenshot({ path: path.join(out, name), fullPage: true });
    captures.push(name);
  }
  const production = process.env.SHIXU_PREVIEW_URL || "http://127.0.0.1:1423";
  await page.goto(production + "/?window=vault");
  await page.getByRole("heading", { name: "密码库已锁定" }).waitFor();
  await page.evaluate(() => document.fonts.ready);
  await capture("D3-production-locked.png");
  await page.getByRole("button", { name: "忘记主密码" }).click();
  assert.match(await page.locator(".vault-help").innerText(), /无法找回/);
  await capture("D3-production-forgot.png");
  await page.getByLabel("主密码", { exact: true }).fill("synthetic-master");
  await page.getByRole("button", { name: "解锁密码库", exact: true }).click();
  await page.getByRole("status").filter({ hasText: "暂不可用" }).waitFor();
  assert.equal(await page.locator("[data-entry-id]").count(), 0);
  assert.equal(
    await page.getByRole("heading", { name: "密码库已锁定" }).count(),
    1,
  );
  assert.equal(
    await page.getByLabel("主密码", { exact: true }).inputValue(),
    "",
  );
  await capture("D3-production-unsupported.png");
  await page.getByRole("button", { name: "新建密码库", exact: true }).click();
  await page.getByRole("heading", { name: "新建密码库" }).waitFor();
  assert.equal(await page.locator('input[type="password"]').count(), 2);
  await capture("D3-production-create.png");
  await page.setViewportSize({ width: 390, height: 844 });
  assert.equal(
    await page.evaluate(() => document.documentElement.scrollWidth),
    390,
  );
  await capture("D3-production-narrow.png");
  const harness = process.env.SHIXU_VAULT_HARNESS_URL;
  if (harness) {
    await page.setViewportSize({ width: 860, height: 850 });
    await page.clock.install();
    await page.goto(harness);
    assert.match(await page.locator("body").innerText(), /仅 UI 合成测试/);
    await page.getByLabel("主密码", { exact: true }).fill("synthetic-master");
    await page.getByRole("button", { name: "解锁密码库", exact: true }).click();
    await page.locator("[data-entry-id]").first().waitFor();
    assert.equal(await page.locator("[data-entry-id]").count(), 2);
    await page.evaluate(() => document.fonts.ready);
    await capture("D3-test-only-unlocked.png");
    await page
      .getByRole("button", { name: "显示密码", exact: true })
      .first()
      .click();
    await page.getByText("synthetic-ui-only", { exact: true }).waitFor();
    await page.clock.fastForward(15000);
    assert.equal(
      await page.getByText("synthetic-ui-only", { exact: true }).count(),
      0,
    );
    await page
      .getByRole("button", { name: "编辑", exact: true })
      .nth(1)
      .click();
    assert.deepEqual(
      await page
        .locator(".vault-form label")
        .evaluateAll((labels) => labels.map((l) => l.firstChild.textContent)),
      ["渠道", "账号", "密码"],
    );
    await capture("D3-test-only-edit.png");
    await page.getByRole("button", { name: "取消", exact: true }).click();
    await page
      .getByRole("button", { name: "删除", exact: true })
      .first()
      .click();
    await page.getByRole("dialog", { name: "确认删除" }).waitFor();
    assert.equal(await page.locator("[data-entry-id]").count(), 2);
    await page.getByRole("button", { name: "取消", exact: true }).click();
    await page.getByRole("button", { name: "锁定密码库", exact: true }).click();
    assert.equal(await page.locator("[data-entry-id]").count(), 0);
  }
  assert.deepEqual(errors, []);
  fs.writeFileSync(
    path.join(out, "D3-browser.json"),
    JSON.stringify(
      {
        production,
        harness: harness || null,
        captures,
        errors,
        productionUnlock: "UNSUPPORTED",
        systemClipboard: "NOT_TESTED",
        harnessScope: "synthetic UI only",
      },
      null,
      2,
    ),
  );
  await browser.close();
  console.log(
    "D3 browser assertions passed; real engine/system clipboard remain unavailable",
  );
})().catch((e) => {
  console.error(e);
  process.exit(1);
});
