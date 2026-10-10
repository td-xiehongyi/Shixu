# 最小首次运行：三字段密码库与纯文字QQ通知入历

2026-10-10，当前仍 **BLOCKED**。目标先接通“渠道、账号、密码”和“获准群纯文字→持久日历”。云端能补代码、做合成portable测试；用户自己的Windows能做真实系统/组件验收。准备机器与安装组件不能代替缺失代码，纯文字子集通过也不能关闭完整首版的12门槛。

## 按依赖补代码

1. **G1固定协议先于V2/V3。** 当前 `crates/shixu-native/src/vault/mod.rs` 只有clipboard；`src-tauri/src/commands.rs` 对全部vault命令只验证载荷并返回Unsupported。没有真正的生产VaultEngine、私有引擎会话、KDBX创建/打开/读取/写入/原子提交/备份、换密重新打开验证或本机秘密会话销毁。先为选定且记录实际版本的引擎定义并实现秘密仅走私有通道的协议与合成G1探针；UUID、Unicode、前后空白、换行、错误密码/篡改、冲突/磁盘满、旧备份主密码必须验证。G1探针/脚本目前是待开发代码，仓库没有可以交给用户立即运行的G1脚本。通过固定协议探针后，才实现V2/V3并接通vault dispatcher，15秒显示/5分钟锁定/迟到响应丢弃与复制权限合并验收。
2. **独立决定QQ组件和客户端版本，再做G2。** `qq/transport.rs` 只有ReceiveTransport规范，`qq/adapter.rs` 消费归一化消息；不是任何组件的真实wire协议。当前组件、精确release、QQ客户端及协议未决定。需实现字面loopback receive-only transport、身份/能力验证、断线/登录过期/补拉、原件下载，并把配置/令牌的本机秘密保护和worker连接接通。`src-tauri/src/runtime.rs` 的WorkerPorts仅配parser/backup，receiver/model保持默认None；不能从normalized合成消息测试推出实际QQ接收成功。G2探针/脚本同样尚未开发。组件版本决定与G2协议验证之后，用一个普通获准合成测试群做实际接收→持久化→规则→日历，不加逐条确认。完整G2仍需≥100条、≥8小时、持久化P95≤5秒/规则P95≤10秒与各格式原件证据。
3. **连接Windows宿主和存储。** 已写DPAPI/ACL适配器、日历SQLite/备份、tray/window与生命周期抽象；runtime使用UnavailableVault，文件锁仅作第二进程排除。需真实会话锁/电源通知、跨进程聚焦、OS登录自启、打包安装/卸载及WebView权限连接。`--login-start`分支并不证明注册自启；生命周期抽象测试并不证明收到Windows通知。日历打开DPAPI失败会退回Unavailable AppState，也需本机可见错误/恢复验收。保护缓存与parser输入交接仍不完整。
4. **复制写边界已有源码，仍待接通和实测。** SystemClipboard在Windows用nonnull owner HWND同步发布三个排除格式后写CF_UNICODETEXT，非WindowsUnsupported。UTF-8无内嵌NUL、最多65536字节；超范围/非文本失败，无截断。准备/Open失败保留旧内容；Empty成功后更晚失败可能丢失旧内容/留下部分格式，Close或owner清理报错时甚至已留下文本。Windows拥有成功发布内存；只有未发布缓冲和暂存内存清零。复制后内容保留至用户覆盖/手动清除；锁库/退出清瞬时秘密但不清剪贴板。固定接口不是任意字节承诺，clipboard仍written_untested/BLOCKED。真实测试命令与手工边界见 [Windows门槛](windows-release.md)。
5. **合成端到端验收。** 用虚构三字段密码库进行创建→解锁→保存→重开→显示/复制→锁库/换密/备份恢复；另一条独立链做纯文字实际群消息→持久来源→自动入历→改期/取消→编辑/撤销→重启不重放。两条链并行运行不应让QQ/日历刷新密码库闲置计时。全部是合成数据，失败/断连不能反馈成功。

## 12门槛：代码与机器分别缺什么

`src-tauri/src/release.rs::policies()` 是完整发布门槛来源；当前清单仅描述缺口，不是新验收证据。缺证据报告保持BLOCKED；质量失败有真实证据时可判FAIL，不能用BLOCKED掩盖失败。

| 门槛 | 代码状态/云端剩余工作 | 用户机器或决定 | 最小链关系 |
| --- | --- | --- | --- |
| g1_vault | 未实现引擎/固定私有协议、G1探针、V2/V3原子加密提交/备份、dispatcher | 引擎实际版本/哈希；Windows合成UUID/Unicode/换行/错误密码/冲突/换密旧备份往返 | 密码链前置，安装本身不足 |
| g2_qq | 通用OneBot11纯文字wire与生产receiver工厂已实现并经本机合成socket测试；Windows UI配置/本机令牌保护接线、重连/附件及G2探针仍缺失 | 明确组件/客户端/协议；本机独立登录；获准普通测试群；100条8小时及附件 | 真实群验收仍前置；本机合成socket不等于G2 |
| windows_shell | 部分tray/window/锁排除/生命周期代码；通知钩子、跨进程聚焦、登录自启、包仍缺 | 真实Windows/WebView/IPC授权、托盘退出/电源/登录/安装运行 | 两条链所需，未实测 |
| windows_storage | 部分DPAPI/ACL/数据库代码；保护缓存/交接与生产恢复缺口 | 两个真实Windows身份；ACL/reparse/占用/磁盘满/崩溃恢复 | 两条链所需，未实测 |
| clipboard | write-only Windows源码written_untested；vault命令未连接，字节兼容受限 | 真实粘贴/持久性/覆盖/手清、历史/云排除/权限/闲置 | 密码链所需，BLOCKED |
| parser_isolation | 真实隔离进程/Job Object限制、无网络/库访问/输入保护交接缺失 | Windows真实512MiB/超时/关闭杀进程/子进程限制 | 超出纯文字链，完整发布仍BLOCKED |
| whole_flow | vault及Windows QQ UI配置未连通；生产receiver工厂已证合成socket→worker→来源→日历，真实桌面端到端未证 | 合成密码库、来源入历/改期/撤销/重启/恢复真实往返 | 最小链要收集子集证据；不等于全门槛PASS |
| quality_text | portable冻结语料已有157例132gold；TP123 FP3 FN9，P97.619% R93.182%，含糊猜测0 | 为实际source/artifact/build重新绑定全语料证据 | 文字指标已达阈值；不是QQ实际接收证据 |
| quality_image | portable解析/OCR接缝已有；真实引擎/定位/质量OPEN | 后续Windows引擎与完整冻结图像集 | 超出最小链，完整发布BLOCKED |
| quality_pdf | portable解析/OCR接缝已有；真实PDF/OCR/质量OPEN | 后续Windows引擎与完整冻结PDF集 | 超出最小链，完整发布BLOCKED |
| quality_docx | portable解析已有，完整事件P75% R35.29%不足 | 改善完整语料并做Windows隔离验收 | 超出最小链，完整发布BLOCKED/质量未达 |
| quality_xlsx | portable解析已有，完整事件P57.14% R30.77%不足 | 改善完整语料并做Windows隔离验收 | 超出最小链，完整发布BLOCKED/质量未达 |

任何原生门槛都不因ignored、mock、cfg检查或操作员自报PASS而通过；clipboard不能改成native_implemented。完整格式质量须按各类型全部冻结gold报告P≥95%/R≥90%与含糊猜测0，保留unsupported正例分母，不因优先文字删除其他门槛。云端可选模型默认关闭，与最小链无强制依赖。

## 用户最少准备

这是一份后续由用户执行的准备清单；此次没有安装、下载、登录、读取真实密码或修改远端。

- 自己的Windows10/11 x64测试机、Git、MSVC“Desktop development with C++”工作负载和Windows SDK、Rust **1.99.0 MSVC**、Node **24.19.0**、pnpm **11.19.0**。核对仓库固定工具链；Windows组合尚未验收。记录实际Windows构建号、身份、各工具`--version`及MSVC/SDK版本。官方环境要求见 [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/#windows)。
- 记录真实WebView2 Runtime版本；缺失时在用户明确授权后独立按 [微软分发说明](https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/distribution) 准备。当前bundle.active=false；不能把Tauri build当已有安装包。
- 只有用户授权后才独立从 [KeePassXC官方Windows页](https://keepassxc.org/download/#windows) 准备组件。确定实际支持的固定release，不在这里指定引擎版本。对将用于G1的GUI/CLI/helper分别记录`--version`、来源/release/tag/许可、可执行文件SHA256并核对官方摘要/签名；安装后不要立即用真实密码库。用户无需提交任何主密码、KDBX、密钥或真实秘密。`Get-FileHash -Algorithm SHA256 -LiteralPath <实际组件文件>`可记录文件身份，摘要不等于协议验收。
- 自己决定QQ组件及QQ客户端的精确版本/协议。仅从选定项目的官方仓库/release/文档确认兼容版本、接收能力及许可，核对release/tag/commit、官方校验/签名（有提供时）与本地SHA256，保留文档URL和查验日期；项目未选定，所以此文不猜官方URL、固定版本或安装命令。准备一个获准普通合成测试群与本机登录；令牌只在本地保护配置，禁止发到对话或放进argv/env/日志。通用OneBot11纯文字receive-only transport已写并通过本机合成socket验证；Windows UI安全令牌配置接线、真实组件/群及G2尚未验证，不能宣称实际QQ已接收。
- 为Windows存储验收准备第二个独立Windows身份；为剪贴板验收准备可覆盖的合成测试剪贴板和历史/云测试设置。记录组件/系统设置与命令直接退出码；测试日志不包含秘密。

安装KeePassXC只提供应用/组件，不实现Shixu私有协议或create/open/read/write/atomic commit/backup/native dispatch。2026-10-10查阅上游 [develop Utils.cpp](https://raw.githubusercontent.com/keepassxreboot/keepassxc/develop/src/cli/Utils.cpp) 的getPassword使用readLine；由此推断stock交互CLI的多行秘密通道有待验证，不能视为结构化字节安全SDK。develop是可变分支，既不是用户安装版本探针，也不是Windows G1已FAIL的证据。必须为实际选定固定版本设计并实测协议；不编造CLI输出formatter或把“装好再试”当完成V2。

只在缺失实现补齐、固定协议/版本确认且本机合成验收有证据后，才收集同构建source/artifact绑定报告。当前 `scripts/verify-windows-release.ps1` 只运行报告校验器，不执行G1/G2探针、安装/登录/测试或代替原生验收。
