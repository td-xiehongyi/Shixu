# 正式密码库准入检查（2026-10-11）

当前状态：本机正式入口已接通，已补齐用户选定范围内的真实密码存储安全验证。当前 Windows 剪贴板历史和云同步关闭；必要增量检查通过，数据故障恢复暂缓，全产品发布仍 BLOCKED。

## 最新：真实账号密码的本机安全验证

用户授权补齐必需安全验证，并明确选择保持剪贴板历史和云同步关闭，只验当前配置与程序排除标记。以下全部使用公开合成数据，没有选择用户主密码、读取正式库内容或既有剪贴板文字，没有更改系统隐私设置。证据目录：`docs/verification/local/vault-security-20261011/`。

| 检查 | 当前证据与结果 |
| --- | --- |
| 第二真实 Windows 身份隔离 | xie 与 CodexSandboxOffline 的实际 SID 不同；双方均可读公共对照。所属账户可申请测试密文的读/写/删除/改权限句柄及目录访问；第二账户对应 7 项均为 Win32 ERROR_ACCESS_DENIED=5，包含正式目录列举。只申请 OPEN_EXISTING 句柄，不执行写入、删除或改 ACL。`access-owner.json`、`access-other.json`。 |
| 当前剪贴板配置 | Windows WinRT API 实测 IsHistoryEnabled=false、IsRoamingEnabled=false；按用户选择保持关闭。`current-config-result.json`。 |
| 实际 Win32 排除标记 | 新增原生验收测试 1 passed；账号、密码两种合成文本经真实 SystemClipboard 发布，三个注册格式均存在且为 DWORD 0，Unicode 文本格式存在。不读取正文。`clipboard-native.log`。 |
| 完全退出使用的关闭方法 | 新增桌面库测试 1 passed，使用上一轮正式构造器生成的合成库。解锁后确认一个真实 node.exe 子进程存活，调用托盘退出所用 VaultController::shutdown，返回后该进程不存在、会话权限撤销且 List 拒绝。`shutdown-native.log`。这验证实际关闭方法，不冒充托盘菜单人工点击。 |
| 不重复验证与数据保全 | 固定 crypto、Windows 资源清单哈希和原生 Windows 边界测试模块之前的内容与已验收基线一致。复用加密持久化/错密拒绝、原生文件与网络隔离、真实父退出 Job 终止、真实 junction 叶与祖先拒绝及适用的桌面证据；未重跑完整边界、换密、闲置或全工作区。测试密文哈希未变。`evidence-reuse.json`。 |

三个排除格式为 `ExcludeClipboardContentFromMonitorProcessing`、`CanIncludeInClipboardHistory`、`CanUploadToCloudClipboard`；Windows 官方说明分别控制监视处理、历史及跨设备同步，后两者 DWORD 0 表示排除。见 [Microsoft Clipboard Formats](https://learn.microsoft.com/en-us/windows/win32/dataxchg/clipboard-formats)。只对当前关闭配置及实际标记给出通过结论；不声称已验历史开启后的列表、远端云同步或第三方剪贴板工具。剪贴板仍按用户既定要求保留到覆盖或手动清除。

测试诊断：观察方首次 OpenClipboard 遇到错误 5，加入仅观察方最多 2 秒等待后捕获短暂占用并通过，生产 writer 未改；退出测试最初受运行 EXE 文件占用影响，改为仅编译 lib 测试。新夹具混合分隔符被严格路径准入拒绝，改为逐组件构建路径后通过；未放宽产品规则。

本轮只增加两个有实际副作用、默认 ignored 的专项验收测试和 test-only 模块接线；业务代码、资源、运行 EXE 均未改变。Rust 格式检查通过。当前 EXE SHA256 仍为 `e8afc3102c8e16a7c4aefa1c5db69f4c1a8c460ffd08568a54b69f123f6a03d8`，正式 KDBX 检查时尚未创建。可以在当前本机配置下从普通入口自行设置主密码、保存真实账号密码。

范围外或仍未验证：故障恢复（包含真实磁盘满、物理断电）按用户要求暂缓；完整符号链接夹具仍缺权限，已有真实目录联接拒绝证据不等同全夹具通过。托盘菜单人工点击、开启历史/跨设备同步、安装包和全产品发布不在本次通过范围。当前没有已发现但未解决的本轮存储安全缺陷。未提交、推送、安装或发布。

## 上一阶段：本机正式入口接通

`KdbxWebEngine::prepared` 的 Windows 分支现调用既有 `Store::prepared`，保留资源精确哈希、ACL/MIC、reparse、固定 Node、AppContainer/Job 和私有管道检查；没有改用便携引擎或放宽权限。普通启动仍固定使用 `%LOCALAPPDATA%\app.shixu.desktop\vault`，虚构入口仍使用 EXE 旁的独立目录。只在虚构模式显示测试数据提示。未选择或保存用户主密码，正式 KDBX 留待用户创建。

本轮仅执行必要验证，证据目录 `docs/verification/local/vault-local-use-20261011/`：

- 原生正式构造器回归：修改前在有效资源下因入口 `Unsupported` 失败；接通后 1 passed / 0 failed。测试只使用自建临时目录，覆盖无效资源不创建目录、创建、保存、加密内容不含明文、关闭重开、错误主密码拒绝、拒绝覆盖既有库。
- 真实普通 Windows 桌面 3 项通过，`normal-entry-result.json` 为 complete=true：正式引擎返回 LOCKED 而非 Unsupported、新建表单空白且可操作、正式目录初始化且原虚构库 SHA256 不变。该检查没有在正式目录填写测试主密码、创建 KDBX 或写入测试条目。
- 类型检查、前端构建、Windows custom-protocol 桌面构建和 Rust 格式通过；两个更新后的 PowerShell runner 语法解析通过。前端构建保留既有分包体积提示。未重跑全工作区、完整原生边界或五分钟闲置回归。
- 当前 EXE SHA256：`e8afc3102c8e16a7c4aefa1c5db69f4c1a8c460ffd08568a54b69f123f6a03d8`；旧已验收 EXE 和修改前源码保留于本轮证据目录 `baseline/`。

使用：双击根目录 `启动拾序.cmd` → 密码库 → 新建密码库，自行设置主密码。库文件为 `%LOCALAPPDATA%\app.shixu.desktop\vault\active\vault.kdbx`；之后重启应用仍使用该目录。第二身份、当前剪贴板排除及原生关闭方法已由上方增量验证接替；数据故障恢复继续暂缓。真实 QQ、附件与安装包不纳入本次验证；未提交、推送、安装或发布。

## 历史：接通前的准入准备

以下保留此前本机接入准备结果。正式入口 Unsupported 已由上方新结果替代；旧点击与换密故障后续已由 [点击与换密修复报告](vault-click-master-fix-2026-10-11.md) 关闭，不能再作为当前未定位故障。

## 阻断原因与本轮修改

1. `KdbxWebEngine::prepared` 的 Windows 分支有明确的正式准入阻断，并不是缺少 KDBX 运行资源。原测试只传入不存在的目录，不能区分资源不存在与准入阻断。本轮改为使用完整、通过固定清单校验的真实资源，并验证被阻断时不创建存储、不改写已有虚构数据。阻断继续保留。
2. 普通启动将 Tauri 的 `resource_dir` 直接传给严格 Win32 路径准入。Tauri 可返回本地磁盘路径的 verbatim 前缀，而原生路径策略拒绝该前缀。本轮新增仅供可信原生启动调用的 `bundled_resources_directory`，去除这一前缀后仍执行原有完整路径校验；普通启动与虚构启动共用此适配器。远程、设备、相对、目录别名路径继续拒绝，原生文件句柄、ACL/MIC、资源哈希与 reparse 校验未放宽。
3. 原生测试在受限工具环境内失败：资源祖先句柄准入失败，AppContainer profile 创建返回 `0x80070002`。同一份代码和测试在普通 Windows 用户权限下，持久化加密库创建/重开/换密测试通过。记录受限运行失败，不把它当产品回归，也不通过放宽隔离来使测试通过。新增日志只包含固定阶段和数值错误码，不包含路径、账号、主密码或条目内容。

## 解锁行为与数据边界

本轮未改变主密码、会话和生命周期实现：本次进程内解锁一次，切页、最小化、关窗到托盘继续保持；手动锁定、完整退出/重启后重新解锁。闲置锁库、系统锁屏/睡眠钩子和剪贴板自动清理不恢复。

所有原生/桌面操作仅使用测试自建数据和既有专用虚构 KDBX。不启动普通模式访问正式数据，不连接真实 QQ，不安装、提交、推送或发布。旧 EXE、既有工作区改动和虚构库已保留。本轮代码差异相对于执行前备份审阅，不将之前未提交的实现算作本轮新增。

## 验收与证据

本轮证据目录：`docs/verification/local/vault-production-20261011-110050/`（忽略目录）。

- 资源路径红绿回归：先 2 failed，修复后 2 passed；见 `path-red.log` / `path-green.log`。
- 正式准入阻断与数据不改写：使用有效固定资源通过；见 `production-gate.log`。
- 真实普通用户加密库持久化：1 passed，包含新建、四字段保存、错误主密码、释放后重开、换密与第二实例拒绝；见 `native-standard-user.log`。
- 补充第二次换密落盘回归：1 passed，48.40 秒；重开后编辑、B→A 换密、关闭再重开，确认 B 被拒绝、A 可打开且编辑描述保留。见 `reverse-master-persistence-final.log`。该测试增量在全工作区回归之后加入，另做针对运行、严格 Clippy 与静态复核；未声称重新运行全工作区。
- 目录联接：真实 Windows 夹具通过，普通目录祖先成功作对照，目录联接的叶和祖先均被拒绝；完整普通用户回归实际执行该测试并通过。目录联接证据不能代替尚不可创建的符号链接文件夹具。
- 固定原生边界：普通用户权限下 1 test passed，169.54 秒，33 类通过 / 4 类阻断；见 `native-boundary-standard-user.log` / `native-boundary-result.json`。包含实际进程权限、网络拒绝、资源完整性、加密检查点、取消、父退出和超时等检查；阻断列表保持原样。
- 最终完整 Rust 工作区：504 passed / 21 failed / 15 ignored，直接退出 101。失败目标仍为 `dpapi_restore`（1）、`parser_isolation`（15）和 `image_pdf`（5）；分别为 Windows 附件缓存不支持及缺固定解析引擎/模型，与本轮路径适配无关。完整失败用例名称见 `rust-failures.json`，原始结果见 `rust-workspace-standard-user.log`，不称全套通过。受限环境额外的 3 项加密库准入失败在普通用户环境复验通过，受限日志保留。
- 前端 186 passed；类型检查、全工作区严格 Clippy、Rust 格式与 `custom-protocol` 桌面构建均 exit 0。源资源和桌面运行资源各 198 项，文件集合及哈希全部匹配固定清单。发布 CLI exit 2，12 项仍 BLOCKED。
- 会话/生命周期/界面 6 个关键文件哈希与上一轮已验收记录一致；见 `unlock-source-preservation.json`。桌面构建时源码指纹见 `final-runtime-source-sha256.json`，补充测试后见 `final-workspace-source-sha256.json`；两者仅 `windows.rs` 的测试段不同，生产实现未再变化。
- 桌面首轮：13 项实际功能检查通过后，整窗截图因测试窗口不在前台而失败，直接 exit 1；保留 `desktop-result.json` 与原始日志。后续页面截图也出现渲染超时。最终功能复测显式跳过截图采集，所有 IPC、加密、持久化、原生窗口操作与 310 秒闲置断言保持；不授予本轮视觉验收。
- 桌面换密：`desktop-regression/` 与 `desktop-traced/` 两次完整流程均在换密后等待成功提示超时，画面显示固定 `AUTH_FAILED` 对应文案；原库仍可用原主密码打开。最初尝试替换 Tauri 的不可写 invoke 函数没有采集到请求，空 trace 不能证明未调用。随后 CDP 断点观察到 B→A 请求并成功（该轮 17 项后因页面截图超时退出）；无断点专门 A→B→A 对照也通过，每次旧密码拒绝、新密码可用，见 `desktop-roundtrip/`。失败原因仍未确定，不放宽换密校验、不添加延时作为修复，不将后续通过视为问题已修复。
- 无截图、无断点完整功能复测：22 项通过，真实闲置 310003ms 后原生列表与界面仍保持解锁；随后锁定按钮点击因被 HTML 截获、超出视口而超时，exit 1 / complete=false，最后一项删除后再次解锁确认未完成。见 `desktop-functional/`。手动锁定与重开已在该轮较早步骤通过，但不以之替代末尾未完成的断言，不称完整 23 项通过。视口/指针失败原因未定位，不使用强制点击或 DOM 事件冒充真实交互验收。
- 最后一次无断点 B→A 完整前置流程复测：13 项通过，exit 0，监听记录确认请求为 B→A、响应无固定错误，旧密码被拒绝、新密码解锁成功；见 `desktop-change-diagnostic/`。它不包含后续窗口/闲置/删除断言，不能代替完整闭环。当前专用虚构库回到脚本 fixture A，历次诊断留下的虚构条目保留，未重置原库。
- 独立只读审阅：未发现 Critical；一项祖先目录负对照不足的 Important 已修复并复核，无剩余问题。静态审阅不能代替运行验收。

## 仍需外部条件

第二个真实 Windows 身份、真实磁盘满、物理断电恢复和完整符号链接夹具权限尚未满足。不将故障注入、进程终止或目录联接视为上述验收通过。正式 Windows 构造器仍返回 `Unsupported`；本轮完成本机准备与核验，不宣称正式密码库已经开放，也不更改 12 项全产品发布门槛。

本机额外待办：定位上述偶发桌面换密失败及闲置后的视口/点击失败，并完成可重复的完整回归；不能只凭全新原生库或一次桌面成功解除该项。本轮不修改此处产品行为，也不将未定位问题归因为用户输入或工具环境。

本轮实际功能复测 EXE SHA256：`bd9c7bb78718edd36db4ebc5d8f69c2b7684fa9568fc44eb62200e2713f33d93`；构建时 source SHA256：`a53e6f04ffa14730cd461c8282f643debe6e6221230b4326de609cf7dafe39b5`。旧 EXE 保留于证据目录 `baseline/`，本轮构建副本为 `verified-desktop.exe`；不得把后补测试/报告当作同一完整源码指纹。

现有 `scripts/test-windows-vault-boundary.ps1` 要求已提交的干净工作区，本轮未获提交授权，因此直接执行对应的固定 Cargo 测试并记录当前源码/产物身份，未取消脚本的干净工作区要求。外部环境具备后须在同一版本上补验，不能复制旧报告解除阻断。
