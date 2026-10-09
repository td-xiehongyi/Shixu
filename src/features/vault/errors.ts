import { BridgeError } from "../../contracts/bridge";
export function vaultError(error: unknown): string {
  const code = error instanceof BridgeError ? error.code : "UNSUPPORTED";
  return {
    LOCKED: "密码库已锁定，请重新解锁。",
    AUTH_FAILED: "主密码错误，密码库仍已锁定。",
    UNSUPPORTED: "密码引擎或系统能力尚未验证，暂不可用。",
    CONFLICT: "条目已发生变化，请重新查看后再保存。",
    STORAGE_FULL: "存储空间不足，未保存。",
    DISCONNECTED: "服务未连接，请稍后重试。",
    PARSE_FAILED: "无法读取数据。",
    INVALID_INPUT: "请检查输入内容。",
  }[code];
}
