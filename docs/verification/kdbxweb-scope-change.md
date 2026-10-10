# 2026-10-10 authorized engine scope change

User choice: KdbxWeb independent process; no newlines in master/account/password. This supersedes KeePassXC CLI routing. Stage1 implements private typed native engine with real synthetic KDBX4 and prepared fixed resources; UI integration follows independent review. Main/calendar/QQ/model receive no vault capability or decrypted database. Existing mask15s/idle5min/revoke/local best-effort cleanup and no automatic clipboard clear remain.

G1, Windows native isolation/ACL/reparse/atomic replacement, GUI interoperability, ten-backup/restore, and release remain BLOCKED. bundle.active stays false. No real credentials are authorized.

本阶段仅支持自有平坦三字段、未压缩 KDBX4/AES/Argon2id19。压缩库在解压前返回 Unsupported；不承诺任意标准 KDBX 导入/互操作。初始KDF为64MiB、3次、1通道；接受上限64MiB、4次、1通道，文件8MiB、帧1MiB、字段64KiB、条目1000、请求30秒。Windows性能及隔离门槛仍未验证。

2026-10-10 backend审阅修正：除外层 gzip 外，XML Binary 元素及 Compressed 属性在返回上游对象加载前拒绝，避免附件解压。自有库额外要求完整列表 JSON 响应（含协议外壳、最长请求ID及转义）≤1MiB；open及保存前检查，超限返回 Unsupported，不写入当前库。每字段64KiB/条目1000/文件8MiB仍非单独可接受保证。Windows/发布与UI接线门槛仍阻断。
