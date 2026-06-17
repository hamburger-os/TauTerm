import { useState, useMemo } from "react";
import { useTranslation } from "react-i18next";
import type {
  TransferHistoryItem as THItem,
  HistoryFilter,
} from "../../../types/transfer";
import { DEFAULT_HISTORY_FILTER } from "../../../types/transfer";
import GlassButton from "../../common/GlassButton";
import HistoryFilters from "../history/HistoryFilters";
import HistoryItem from "../history/HistoryItem";
import styles from "../FileTransferPanel.module.css";

interface HistoryColumnProps {
  items: THItem[];
  onClear: () => void;
}

/**
 * 右列：过滤器 + 历史记录列表
 */
export default function HistoryColumn({
  items,
  onClear,
}: HistoryColumnProps) {
  const { t } = useTranslation();
  const [filter, setFilter] = useState<HistoryFilter>(DEFAULT_HISTORY_FILTER);

  const filtered = useMemo(() => {
    return items.filter((item) => {
      if (filter.protocol !== "all" && item.protocol !== filter.protocol)
        return false;
      if (filter.direction !== "all" && item.direction !== filter.direction)
        return false;
      if (filter.status !== "all" && item.status !== filter.status)
        return false;
      return true;
    });
  }, [items, filter]);

  return (
    <div className={styles.historyColumn}>
      <div
        style={{
          display: "flex",
          justifyContent: "space-between",
          alignItems: "center",
          paddingBottom: "4px",
          borderBottom: "1px solid var(--glass-border)",
          flexShrink: 0,
        }}
      >
        <h4 className={styles.columnTitle} style={{ border: "none", padding: 0 }}>
          {t("transfer.history")}
        </h4>
        {items.length > 0 && (
          <GlassButton variant="ghost" size="sm" onClick={onClear}>
            {t("transfer.clearHistory")}
          </GlassButton>
        )}
      </div>

      <HistoryFilters filter={filter} onChange={setFilter} />

      {filtered.length === 0 ? (
        <div
          style={{
            flex: 1,
            display: "flex",
            alignItems: "center",
            justifyContent: "center",
            fontSize: "var(--text-xs)",
            color: "var(--text-muted)",
          }}
        >
          {items.length === 0
            ? t("transfer.noHistory")
            : t("palette.noResults")}
        </div>
      ) : (
        <div className={styles.historyListScroll}>
          {filtered.map((item) => (
            <HistoryItem key={item.id} item={item} />
          ))}
        </div>
      )}
    </div>
  );
}
