# 最小首次运行：三字段密码库与纯文字QQ通知入历

2026-10-10，当前仍 **BLOCKED**。目标先接通“渠道、账号、密码”和“获准群纯文字→持久日历”。云端能补代码、做合成portable测试；用户自己的Windows能做真实系统/组件验收。准备机器与安装组件不能代替缺失代码，纯文字子集通过也不能关闭完整首版的12门槛。

本机更新：提交 `c638668` 的固定 Windows 最小工作流已实际通过，包括合成 KDBX 生命周期、隔离边界和 MSVC 桌面构建；真实窗口完成通知注册，用户确认密码库可以点击、托盘可以退出，进程 exit 0。锁屏/休眠待验，退出时 WebView 注销错误 1412 待定位。生产构造器仍 Unsupported，真实桌面密码接线、剪贴板、QQ 与安装仍未验收。最新证据和明确阻断见 [本机阶段报告](windows-local-2026-10-10.md)；以下完整门槛不因此自动通过。

## 按依赖补代码

1. **G1 Windows实际隔离仍前置。** 已接通固定KdbxWeb/Node独立进程、私有协议、VaultService有界actor、命令与独立UI；Linux合成测试覆盖创建/CRUD/重开/换密/错密/篡改/冲突/代际取消。Windows构造器仍Unsupported，需实际Job/token/resource/network/ACL/reparse/原子文件替换及GUI验证；十份加密备份/恢复仍在最小阶段外；剪贴板命令已接线但Windows未验收，不能把Linux证据当Windows生产G1通过。
2. **独立决定QQ组件和客户端版本，再做G2。** 已实现 receive-only OneBot11 `/event` 纯文字 wire、literal loopback 与身份验证，并通过本机合成真实 socket 测试。Windows 设置现有只写令牌/用户级保护保存与显式连接、断开；runtime 已将可管理 receiver 注入 `WorkerPorts.receiver`，model 仍默认 None。当前仅一个活动来源，切换须先断开，保存不连接，配置变更/恢复/重启须显式重新连接，没有自动重连。组件精确 release、QQ 客户端及本机真实群尚未验收，Windows UI/DPAPI/WebView 实际运行仍 written_untested/BLOCKED；缺少 G2 探针、补拉/附件下载和真实端到端证据。用户一次授权群的新文字通知自动入历，不加逐条确认。完整G2仍需≥100条、≥8小时、持久化P95≤5秒/规则P95≤10秒与各格式原件证据。详见[安全连接接线](qq-connection-ui.md)。
3. **连接Windows宿主和存储。** 已写DPAPI/ACL适配器、日历SQLite/备份、tray/window与生命周期抽象；runtime与命令共享failclosed vault controller，Windows引擎不可用时保留日历/QQ，文件锁仅作第二进程排除。需真实会话锁/电源通知、跨进程聚焦、OS登录自启、打包安装/卸载及WebView权限连接。`--login-start`分支并不证明注册自启；生命周期抽象测试并不证明收到Windows通知。日历打开DPAPI失败会退回Unavailable AppState，也需本机可见错误/恢复验收。保护缓存与parser输入交接仍不完整。
4. **复制命令已守卫接到原生写边界，仍待Windows实测。** SystemClipboard在Windows用nonnull owner HWND同步发布三个排除格式后写CF_UNICODETEXT，非WindowsUnsupported。UTF-8无内嵌NUL、最多65536字节；超范围/非文本失败，无截断。准备/Open失败保留旧内容；Empty成功后更晚失败可能丢失旧内容/留下部分格式，Close或owner清理报错时甚至已留下文本。Windows拥有成功发布内存；只有未发布缓冲和暂存内存清零。复制后内容保留至用户覆盖/手动清除；锁库/退出清瞬时秘密但不清剪贴板。固定接口不是任意字节承诺，clipboard仍written_untested/BLOCKED。真实测试命令与手工边界见 [Windows门槛](windows-release.md)。
5. **合成端到端验收。** 用虚构三字段密码库进行创建→解锁→保存→重开→显示/复制→锁库/换密/备份恢复；另一条独立链做纯文字实际群消息→持久来源→自动入历→改期/取消→编辑/撤销→重启不重放。两条链并行运行不应让QQ/日历刷新密码库闲置计时。全部是合成数据，失败/断连不能反馈成功。

## 12门槛：代码与机器分别缺什么

`src-tauri/src/release.rs::policies()` 是完整发布门槛来源；当前清单仅描述缺口，不是新验收证据。缺证据报告保持BLOCKED；质量失败有真实证据时可判FAIL，不能用BLOCKED掩盖失败。

| 门槛             | 代码状态/云端剩余工作                                                                                                                                     | 用户机器或决定                                                               | 最小链关系                               |
| ---------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------- | ---------------------------------------- |
| g1_vault         | Linux真实引擎/私有协议/actor/dispatcher/UI已接通；Windows隔离和十份备份恢复未验收                                                                         | 引擎实际版本/哈希；Windows合成UUID/Unicode/换行/错误密码/冲突/换密旧备份往返 | 密码链前置，安装本身不足                 |
| g2_qq            | OneBot11纯文字wire、Windows UI只写保护配置与runtime receiver已接线并经本机合成socket到Calendar测试；实际Windows written_untested，重连/附件及G2探针仍缺失 | 明确组件/客户端/协议；本机独立登录；获准普通测试群；100条8小时及附件         | 真实群验收仍前置；本机合成socket不等于G2 |
| windows_shell    | 部分tray/window/锁排除/生命周期代码；通知钩子、跨进程聚焦、登录自启、包仍缺                                                                               | 真实Windows/WebView/IPC授权、托盘退出/电源/登录/安装运行                     | 两条链所需，未实测                       |
| windows_storage  | 部分DPAPI/ACL/数据库代码；保护缓存/交接与生产恢复缺口                                                                                                     | 两个真实Windows身份；ACL/reparse/占用/磁盘满/崩溃恢复                        | 两条链所需，未实测                       |
| clipboard        | write-only Windows源码written_untested；vault_copy命令已按epoch/session守卫连接，字节兼容受限                                                             | 真实粘贴/持久性/覆盖/手清、历史/云排除/权限/闲置                             | 密码链所需，BLOCKED                      |
| parser_isolation | 真实隔离进程/Job Object限制、无网络/库访问/输入保护交接缺失                                                                                               | Windows真实512MiB/超时/关闭杀进程/子进程限制                                 | 超出纯文字链，完整发布仍BLOCKED          |
| whole_flow       | vault已接通Linux合成链但Windows不可用；QQ UI保护配置与runtime receiver已接线，已证同save/connect入口的合成socket→worker→来源→日历，真实桌面端到端未证     | 合成密码库、来源入历/改期/撤销/重启/恢复真实往返                             | 最小链要收集子集证据；不等于全门槛PASS   |
| quality_text     | portable冻结语料已有157例132gold；TP123 FP3 FN9，P97.619% R93.182%，含糊猜测0                                                                             | 为实际source/artifact/build重新绑定全语料证据                                | 文字指标已达阈值；不是QQ实际接收证据     |
| quality_image    | portable解析/OCR接缝已有；真实引擎/定位/质量OPEN                                                                                                          | 后续Windows引擎与完整冻结图像集                                              | 超出最小链，完整发布BLOCKED              |
| quality_pdf      | portable解析/OCR接缝已有；真实PDF/OCR/质量OPEN                                                                                                            | 后续Windows引擎与完整冻结PDF集                                               | 超出最小链，完整发布BLOCKED              |
| quality_docx     | portable解析已有，完整事件P75% R35.29%不足                                                                                                                | 改善完整语料并做Windows隔离验收                                              | 超出最小链，完整发布BLOCKED/质量未达     |
| quality_xlsx     | portable解析已有，完整事件P57.14% R30.77%不足                                                                                                             | 改善完整语料并做Windows隔离验收                                              | 超出最小链，完整发布BLOCKED/质量未达     |

任何原生门槛都不因ignored、mock、cfg检查或操作员自报PASS而通过；clipboard不能改成native_implemented。完整格式质量须按各类型全部冻结gold报告P≥95%/R≥90%与含糊猜测0，保留unsupported正例分母，不因优先文字删除其他门槛。云端可选模型默认关闭，与最小链无强制依赖。

## 用户最少准备

这是一份后续由用户执行的准备清单；此次没有安装、下载、登录、读取真实密码或修改远端。

- 自己的Windows10/11 x64测试机、Git、MSVC“Desktop development with C++”工作负载和Windows SDK、Rust **1.99.0 MSVC**、Node **24.19.0**、pnpm **11.19.0**。核对仓库固定工具链；Windows组合尚未验收。记录实际Windows构建号、身份、各工具`--version`及MSVC/SDK版本。官方环境要求见 [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/#windows)。
- 记录真实WebView2 Runtime版本；缺失时在用户明确授权后独立按 [微软分发说明](https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/distribution) 准备。当前bundle.active=false；不能把Tauri build当已有安装包。
- 密码引擎采用预先准备的固定 Node26.11.1/KdbxWeb2.1.1/hash-wasm4.12.0资源，运行时无下载或系统Node回退；来源、SHA256及许可见[kdbxweb-engine](kdbxweb-engine.md)。KeePassXC不再是必装运行组件，可选GUI互操作只用虚构库。
- 自己决定QQ组件及QQ客户端的精确版本/协议。仅从选定项目的官方仓库/release/文档确认兼容版本、接收能力及许可，核对release/tag/commit、官方校验/签名（有提供时）与本地SHA256，保留文档URL和查验日期；项目未选定，所以此文不猜官方URL、固定版本或安装命令。准备一个获准普通合成测试群与本机登录；令牌只在本地保护配置，禁止发到对话或放进argv/env/日志。通用OneBot11纯文字receive-only transport已写并通过本机合成socket验证；Windows UI安全令牌配置接线、真实组件/群及G2尚未验证，不能宣称实际QQ已接收。
- 为Windows存储验收准备第二个独立Windows身份；为剪贴板验收准备可覆盖的合成测试剪贴板和历史/云测试设置。记录组件/系统设置与命令直接退出码；测试日志不包含秘密。

用户选择KdbxWeb；只禁止主密码/current/new/confirmation和存储PASSWORD的CR/LF/NEL/LS/PS换行。ACCOUNT与CHANNEL允许原先字符，不新增账号禁令。UI在paste/drop/beforeinput和提交边界拒绝完整换行载荷，不裁剪、替换或部分粘贴。Windows引擎构造器仍Unsupported，复制/备份不伪造成功，真实主密码无需发送到对话。

只在缺失实现补齐、固定协议/版本确认且本机合成验收有证据后，才收集同构建source/artifact绑定报告。当前 `scripts/verify-windows-release.ps1` 只运行报告校验器，不执行G1/G2探针、安装/登录/测试或代替原生验收。

Stage B 私有 Windows launcher/store 和固定原生probe已写，公开构造器仍Unsupported。用户组件未准备时的精确检查、公共资源准备和标准用户合成测试命令见 [Windows vault边界执行](windows-vault-boundary.md)；云端cfg不是实际Windows结果。
