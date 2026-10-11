# Windows 桌面虚构密码库闭环（2026-10-11）

本文保留加入“描述”之前的产物与验收记录。当前程序、资源版本及留存虚构库 fixture B 见 [描述字段验收记录](vault-description-2026-10-11.md)。

范围：本机标准用户、真实 Windows WebView2/Tauri 窗口、固定 KdbxWeb/Node AppContainer 密码进程、专用虚构 KDBX。正式密码库构造器仍返回 Unsupported，12 项发布门槛不因本轮局部验收关闭。无提交、推送、安装或发布。

## 使用

双击项目根目录 `虚构密码库验收.cmd`，点击“密码库”，用虚构主密码新建或解锁。`.\虚构密码库验收.cmd /check` 仅检查路径。

固定参数 `--vault-fictional` 不接受附加路径或其他选项。数据保留在 EXE 旁 `shixu-fictional-desktop/vault/active/vault.kdbx`；WebView2 数据独立放在同根 `webview`。该模式不初始化正式日历/设置/备份存储、不连接 QQ。普通 `启动拾序.cmd` 仍使用原普通模式。不要填写真实账号、主密码或条目密码。

自动验收生成的库只使用脚本中两组明确标注的虚构主密码，脚本可重复运行，不重置已有库；未知主密码会失败并保留数据。业务使用请在完整准入和发布验收完成后另行准备。

本机本轮留存验收库为 fixture A，虚构主密码 `Fictional-Desktop-Master-2026-A`。本轮新增的条目已在删除测试中移除；可以用该虚构常量解锁后添加自己的虚构条目。它不是真实凭据。

## 实现与恢复

- 虚构模式从 EXE 旁固定资源目录构造路径，避免 Tauri `resource_dir` 的 verbatim 前缀被严格路径策略拒绝；不放宽外部路径校验。
- Windows 当前身份与专用根决定稳定 AppContainer SID；私有库、低完整性 inbox、只读资源分别校验 DACL/owner/完整性，固定资源清单和哈希保持原值。
- 初始化和活动库各有独占 lease。资源完整写入并 flush 后无覆盖原子发布；尚未完成且无 KDBX 的已认证初始化可以恢复。旧截断资源保留为 `recovered-resource-*`，中断 staging 保留在 manifest 外。已有 KDBX 缺少准入 marker 或外来目录拒绝修复；完成后的资源损坏拒绝使用。
- 独立原生控制器保留 epoch/session 撤销、实际 resolver 发布检查、15 秒显示及 5 分钟闲置锁定。锁屏/睡眠/唤醒按用户决定保持范围外。剪贴板不设置自动清除、锁库清除或退出清除。

## 本机验收方法

原始日志、截图与产物身份保存在忽略目录 `docs/verification/local/desktop-fictional-2026-10-11/`。不将个人数据或二进制提交到 Git。

`scripts/test-windows-vault-desktop.cjs` 启动真实 EXE，仅对子进程注入本机 loopback WebView2 CDP 参数，使用已安装 Playwright；没有 mock、浏览器 fixture、计时替换或安装下载。确认虚构窗口 URL 后，操作真实窗口和 IPC、读写实际虚构 KDBX，读取已经由本测试写入的虚构系统剪贴板内容。原生 WM_CLOSE 验证关窗；进程重启测试终止本脚本拥有的进程后重新启动，**不代表托盘菜单退出验收**。普通启动器不启用 CDP。

```powershell
# 已有 VS x64 开发环境、锁定依赖与资源；不自动安装
cargo test -p shixu-native --lib --target x86_64-pc-windows-msvc --locked --offline vault::windows::tests:: -- --nocapture --test-threads=1
cargo test -p shixu-native --lib --target x86_64-pc-windows-msvc --locked --offline vault::windows::tests::synthetic_native_boundary -- --exact --ignored --nocapture --test-threads=1
cargo test --workspace --target x86_64-pc-windows-msvc --locked --offline --features custom-protocol --no-fail-fast -- --test-threads=1
cargo clippy --workspace --all-targets --target x86_64-pc-windows-msvc --locked --offline --features custom-protocol -- -D warnings
cargo build -p shixu-desktop --target x86_64-pc-windows-msvc --locked --offline --features custom-protocol
$env:SHIXU_PLAYWRIGHT_MODULE = '<本机已安装的 playwright 模块目录>'
node scripts/test-windows-vault-desktop.cjs
```

桌面程序运行时 Windows 锁住 EXE，应先结束本脚本的测试实例再执行会重链 EXE 的 workspace/build 命令。测试不关闭用户已有进程。

## 结果与边界

| 检查 | 本轮结果 |
| --- | --- |
| 真实桌面首轮 | 13 项通过；真实闲置 302549 ms；新建、CRUD、遮罩、复制、错密、换密、原生关窗、进程重启、删除后重开 |
| 最终桌面重验 | exit 0，13 项通过，真实闲置 299985 ms；带 CDP 归属校验、固定剪贴板错误，`complete=true` |
| Windows 原生回归 | 针对组 6 passed / 8 ignored；完整工作区内 native lib 8 passed / 8 ignored |
| 显式原生边界 | 33 项通过，4 项阻断：第二身份、物理断电、真实磁盘满、reparse 夹具权限 |
| 实际剪贴板 | 显式 Windows Unicode/排除格式测试 1 passed；不等于历史/云同步 UI 验收 |
| 桌面单元回归 | 18 passed，含启动选择、撤销、晚回复/复制、原生闲置 ticker |
| 前端 | 88 tests passed；类型检查、构建通过（有 bundle 大小提示） |
| Rust 静态检查 | 全工作区、all-targets、MSVC、`-D warnings` Clippy 通过；rustfmt 与 diff check 通过 |
| 完整工作区 | exit 101，494 passed / 21 failed / 15 ignored；3 个既有失败目标保持可见 |
| 全仓前端格式 | exit 1，34 个未修改文件有既有格式问题；本轮唯一修改的 TSX 格式检查通过 |
| 实际发布校验 | exit 2，12 项 BLOCKED；未修改全局 capability 让测试自称完整发布 |

完整工作区的 21 项失败来自 `dpapi_restore`（1）与 `parser_isolation`（15）的 Windows 附件缓存 Unsupported，以及 `image_pdf`（5）缺固定 N4 引擎/模型。本轮没有跳过或修改这些失败测试。

最终 EXE 为 `target/x86_64-pc-windows-msvc/debug/shixu-desktop.exe`，SHA256 `8e45437e179f1cb1bd03337028714971780a1185c8e58eb1de60a5a1d3490ef8`；内嵌 source SHA256 `1d31b29508cea9bcf069b7adc257d4447339e09fb37da4e8a14f93d15c0b0e41`。HEAD 仍 `a76f4ba619ad610c040acff857ba47a558e8d86c`，本轮实现未提交；该构建指纹包含当时的未提交源码，而不是声称产物来自干净 HEAD。本报告结果在测试后补填，不重建已测试 EXE。

忽略证据目录中 `desktop-artifact-manifest.json` 绑定最终完整桌面重验，`implementation-sha256.json` 记录实现/测试/启动器字节。原始、随 EXE 和专用验收根三份资源均为 198 文件、全部匹配原清单。旧 EXE 保留为 `previous-shixu-desktop.exe`；测试不删除原库或重置验收库。两轮日志、实际截图和未关闭门槛均保留。

仍待验收：正式存储准入、不同 Windows 身份、物理断电与真实磁盘满、reparse 夹具权限、当前产物托盘菜单退出、剪贴板历史/云同步 UI、真实 QQ、附件完整质量及安装卸载。原始附件报告丢失 Q1 仍 OPEN。本轮不修改全局 release capability 声明。
