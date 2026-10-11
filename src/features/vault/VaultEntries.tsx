import type { ReactNode } from "react";
import type { VaultSummary } from "../../contracts/domain";
import { MagnifyingGlass } from "@phosphor-icons/react/dist/csr/MagnifyingGlass";
import { Plus } from "@phosphor-icons/react/dist/csr/Plus";
import { Copy } from "@phosphor-icons/react/dist/csr/Copy";
import { Eye } from "@phosphor-icons/react/dist/csr/Eye";
import { PencilSimple } from "@phosphor-icons/react/dist/csr/PencilSimple";
import { Trash } from "@phosphor-icons/react/dist/csr/Trash";
import { Key } from "@phosphor-icons/react/dist/csr/Key";
type Props = {
  rows: VaultSummary[];
  selectedId: string | null;
  search: string;
  busy: boolean;
  visiblePassword: string | null;
  children?: ReactNode;
  onSearch: (value: string) => void;
  onSelect: (row: VaultSummary) => void;
  onNew: () => void;
  onEdit: (row: VaultSummary) => void;
  onDelete: (row: VaultSummary) => void;
  onReveal: (row: VaultSummary) => void;
  onCopy: (row: VaultSummary, field: "account" | "password") => void;
};
export function VaultEntries(p: Props) {
  const selected = p.rows.find((r) => r.entry_id === p.selectedId) ?? p.rows[0];
  return (
    <div className="vault-workspace">
      <aside className="vault-list-pane" aria-label="密码条目">
        <div className="vault-search-row">
          <label className="vault-search">
            <MagnifyingGlass size={21} />
            <input
              aria-label="搜索渠道或账号"
              placeholder="搜索渠道或账号"
              value={p.search}
              onChange={(e) => p.onSearch(e.target.value)}
            />
          </label>
          <button className="primary" onClick={p.onNew} disabled={p.busy}>
            <Plus size={19} />
            新增条目
          </button>
        </div>
        <h2 className="vault-count">条目 · {p.rows.length}</h2>
        <div className="vault-list">
          {p.rows.map((row) => (
            <button
              type="button"
              data-entry-id={row.entry_id}
              key={row.entry_id}
              className={`vault-list-item ${selected?.entry_id === row.entry_id ? "selected" : ""}`}
              aria-pressed={selected?.entry_id === row.entry_id}
              onClick={() => p.onSelect(row)}
              disabled={p.busy}
            >
              <span className="vault-item-icon">
                <Key size={24} />
              </span>
              <span>
                <strong>{row.channel}</strong>
                <small>{row.account}</small>
              </span>
              <span aria-hidden="true">›</span>
            </button>
          ))}
        </div>
        {!p.rows.length && (
          <p className="vault-empty">
            {p.search
              ? "没有匹配的渠道或账号。"
              : "暂无条目，新增一个账号开始整理。"}
          </p>
        )}
      </aside>
      <section className="vault-details" aria-label="条目详情">
        {p.children ??
          (selected ? (
            <>
              <div className="vault-detail-title">
                <h2>{selected.channel}</h2>
                <button
                  data-edit
                  onClick={() => p.onEdit(selected)}
                  disabled={p.busy}
                >
                  <PencilSimple size={19} />
                  编辑
                </button>
              </div>
              <div className="vault-detail-field">
                <span>渠道</span>
                <div className="vault-detail-value">{selected.channel}</div>
              </div>
              <div className="vault-detail-field">
                <span>账号</span>
                <div className="vault-detail-value">
                  <strong>{selected.account}</strong>
                  <button
                    onClick={() => p.onCopy(selected, "account")}
                    disabled={p.busy}
                  >
                    <Copy size={19} />
                    复制账号
                  </button>
                </div>
              </div>
              <div className="vault-detail-field">
                <span>密码</span>
                <div className="vault-detail-value">
                  <code>{p.visiblePassword ?? "••••••••••••"}</code>
                  <div className="vault-inline-actions">
                    <button
                      onClick={() => p.onReveal(selected)}
                      disabled={p.busy}
                    >
                      <Eye size={19} />
                      {p.visiblePassword === null ? "显示密码" : "隐藏密码"}
                    </button>
                    <button
                      onClick={() => p.onCopy(selected, "password")}
                      disabled={p.busy}
                    >
                      <Copy size={19} />
                      复制密码
                    </button>
                  </div>
                </div>
              </div>
              <div className="vault-detail-field">
                <span>描述</span>
                <p className="vault-detail-value vault-description">
                  {selected.description || "暂无描述"}
                </p>
              </div>
              <div className="vault-detail-bottom">
                <button
                  className="vault-danger"
                  onClick={() => p.onDelete(selected)}
                  disabled={p.busy}
                >
                  <Trash size={19} />
                  删除条目
                </button>
              </div>
            </>
          ) : (
            <p className="vault-empty">选择或新增条目以查看详情。</p>
          ))}
      </section>
    </div>
  );
}
