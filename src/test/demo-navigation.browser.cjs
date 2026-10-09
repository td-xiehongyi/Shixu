// Focused built-browser regression. Use an installed Playwright/Chromium; no provider data.
// SHIXU_PLAYWRIGHT_MODULE may point to an externally supplied Playwright installation.
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const { chromium } = require(
  process.env.SHIXU_PLAYWRIGHT_MODULE || "playwright",
);
const preview = process.env.SHIXU_PREVIEW_URL || "http://127.0.0.1:1421";
const evidenceDir = process.env.SHIXU_BROWSER_EVIDENCE_DIR;

(async () => {
  const browser = await chromium.launch({
    executablePath: process.env.SHIXU_CHROMIUM,
    headless: true,
    args: ["--no-sandbox"],
  });
  const page = await browser.newPage({
    viewport: { width: 1487, height: 1058 },
  });
  const errors = [];
  const states = [];
  page.on("pageerror", (error) => errors.push(error.message));
  page.on("console", (message) => {
    if (message.type() === "error") errors.push(message.text());
  });
  if (evidenceDir) fs.mkdirSync(evidenceDir, { recursive: true });
  async function labelVisible() {
    const labels = await page.getByText("演示数据", { exact: true }).all();
    return (await Promise.all(labels.map((label) => label.isVisible()))).some(
      Boolean,
    );
  }
  async function capture(name) {
    if (evidenceDir)
      await page.screenshot({
        path: path.join(evidenceDir, name),
        fullPage: true,
      });
  }
  async function select(module, demo) {
    await page.getByRole("button", { name: module, exact: true }).click();
    assert.equal(
      await labelVisible(),
      demo,
      `${module}: explicit demo label must match mode`,
    );
    const body = await page.locator("body").innerText();
    states.push({
      module,
      demo,
      labelVisible: await labelVisible(),
      notices: await page.locator(".notice").count(),
    });
    if (!demo) {
      assert.ok(
        !body.includes("已选群正在采集"),
        `${module}: simulated collecting must disappear`,
      );
      assert.ok(
        !body.includes("日历与通知正常运行"),
        `${module}: simulated healthy status must disappear`,
      );
      assert.equal(
        await page.locator(".notice").count(),
        0,
        `${module}: synthetic notices must disappear`,
      );
    }
  }
  try {
    await page.goto(`${preview}/?demo=1`);
    await page.evaluate(() => document.fonts.ready);
    assert.equal(
      await labelVisible(),
      true,
      "Calendar starts explicitly labeled",
    );
    assert.equal(await page.locator(".event").count(), 3);
    await capture("D2-fix1-demo-desktop.png");
    await select("通知", true);
    assert.equal(await page.locator(".notice").count(), 3);
    assert.ok(
      (await page.locator("body").innerText()).includes("已选群正在采集"),
    );
    await capture("D2-fix1-demo-notifications.png");
    await select("密码库", true);
    await select("设置", true);
    await page
      .getByRole("button", { name: "今天", exact: true })
      .first()
      .click();
    assert.equal(await labelVisible(), true, "Today keeps demo mode explicit");
    assert.equal(await page.locator(".event").count(), 3);
    await select("设置", true);
    await page.getByLabel("显示演示数据").uncheck();
    assert.equal(
      await labelVisible(),
      false,
      "Turning demo off removes all demo labels",
    );
    for (const module of ["通知", "密码库", "设置", "日历"])
      await select(module, false);
    assert.equal(
      await page.locator(".event").count(),
      0,
      "Synthetic calendar does not leak into actual mode",
    );
    await select("通知", false);
    assert.ok((await page.locator("body").innerText()).includes("QQ 未连接"));
    await capture("D2-fix1-default-notifications.png");
    await page.setViewportSize({ width: 390, height: 844 });
    await page.goto(`${preview}/?demo=1`);
    await page.evaluate(() => document.fonts.ready);
    await capture("D2-fix1-demo-narrow.png");
    await select("通知", true);
    await capture("D2-fix1-demo-notifications-narrow.png");
    const clipping = await page.evaluate(() => ({
      width: innerWidth,
      scrollWidth: document.documentElement.scrollWidth,
    }));
    assert.equal(
      clipping.scrollWidth,
      clipping.width,
      "Narrow notifications must not clip",
    );
    assert.deepEqual(errors, [], "Browser console/page errors must be empty");
    if (evidenceDir)
      fs.writeFileSync(
        path.join(evidenceDir, "D2-fix1-browser.json"),
        JSON.stringify({ states, clipping, errors }, null, 2),
      );
    console.log(
      "PASS: demo labels survive all module navigation; demo-off isolates synthetic notices/status/events; narrow clipping and console checks pass",
    );
  } finally {
    await browser.close();
  }
})().catch((error) => {
  console.error(error);
  process.exitCode = 1;
});
