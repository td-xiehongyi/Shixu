// Test-only browser fixture. Never imported by App or the production index entry.
import { createRoot } from "react-dom/client";
import { VaultWindow, type VaultPort } from "../features/vault/VaultWindow";
import type { VaultSummary } from "../contracts/domain";
import "../styles.css";
import "@fontsource-variable/noto-sans-sc";
let rows: VaultSummary[] = [1, 2].map((n) => ({
  entry_id: `11111111-1111-4111-8111-11111111111${n}`,
  channel: "测试渠道",
  account: `虚构账号${n}`,
  revision: "9007199254740993",
  created_at: 0,
  updated_at: 0,
}));
const port: VaultPort = {
  vaultCreate: async () => {},
  vaultActivity: async () => {},
  vaultUnlock: async () => {},
  vaultChangeMaster: async () => {},
  vaultLock: async () => {},
  vaultCopy: async () => {},
  vaultList: async () => rows,
  vaultReveal: async () => new TextEncoder().encode("synthetic-ui-only"),
  vaultApply: async (value) => {
    if (value.operation === "delete") {
      const old = rows.find((r) => r.entry_id === value.id)!;
      rows = rows.filter((r) => r.entry_id !== value.id);
      return old;
    }
    const old =
      value.operation === "update"
        ? rows.find((r) => r.entry_id === value.id)
        : undefined;
    const saved = {
      entry_id: old?.entry_id ?? crypto.randomUUID(),
      channel: value.channel,
      account: value.account,
      revision: old ? (BigInt(old.revision) + 1n).toString() : "1",
      created_at: 0,
      updated_at: 0,
    };
    rows = old
      ? rows.map((r) => (r.entry_id === saved.entry_id ? saved : r))
      : [...rows, saved];
    return saved;
  },
};
createRoot(document.getElementById("root")!).render(
  <VaultWindow port={port} />,
);
