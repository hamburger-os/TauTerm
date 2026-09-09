import { useTranslation } from "react-i18next";
import type { ToolHistoryEntry } from "../../hooks/useToolInputHistory";
import styles from "./ToolInputHistory.module.css";

interface ToolInputHistoryProps {
  entries: ToolHistoryEntry[];
  onSelect: (value: string) => void;
  onTogglePinned: (value: string) => void;
  onClearRecent: () => void;
}

function preview(value: string): string {
  const compact = value.replace(/\s+/g, " ").trim();
  return compact.length > 44 ? compact.slice(0, 43) + "…" : compact;
}

export default function ToolInputHistory({
  entries,
  onSelect,
  onTogglePinned,
  onClearRecent,
}: ToolInputHistoryProps) {
  const { t } = useTranslation();
  if (entries.length === 0) return null;

  return (
    <details className={styles.history}>
      <summary>
        {t("tools.historyTitle", { count: entries.length })}
      </summary>
      <div className={styles.list}>
        {entries.map((entry) => (
          <div key={entry.value} className={styles.entry}>
            <button
              type="button"
              className={styles.valueButton + " liquid-glass-ghost-button"}
              onClick={() => onSelect(entry.value)}
              title={entry.value}
            >
              <code>{preview(entry.value)}</code>
            </button>
            <button
              type="button"
              className={styles.pinButton + " liquid-glass-ghost-button"}
              onClick={() => onTogglePinned(entry.value)}
              aria-pressed={entry.pinned}
              title={entry.pinned ? t("tools.unpinInput") : t("tools.pinInput")}
            >
              {entry.pinned ? "★" : "☆"}
            </button>
          </div>
        ))}
        <button
          type="button"
          className={styles.clearButton + " liquid-glass-ghost-button"}
          onClick={onClearRecent}
        >
          {t("tools.clearRecentInputs")}
        </button>
      </div>
    </details>
  );
}
