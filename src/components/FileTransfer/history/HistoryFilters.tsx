import { useTranslation } from "react-i18next";
import type {
  HistoryFilter,
  ProtocolType,
  TransferDirection,
  TransferStatus,
} from "../../../types/transfer";
import { PROTOCOL_TYPES } from "../../../types/transfer";

interface HistoryFiltersProps {
  filter: HistoryFilter;
  onChange: (f: HistoryFilter) => void;
}

const selectStyle: React.CSSProperties = {
  padding: "2px 4px",
  fontSize: "0.6rem",
  background: "var(--glass-bg)",
  border: "1px solid var(--glass-border)",
  borderRadius: "var(--radius-sm)",
  color: "var(--text-secondary)",
  cursor: "pointer",
  fontFamily: "inherit",
};

export default function HistoryFilters({
  filter,
  onChange,
}: HistoryFiltersProps) {
  const { t } = useTranslation();

  return (
    <div
      style={{
        display: "flex",
        gap: "4px",
        flexShrink: 0,
      }}
    >
      <select
        style={selectStyle}
        value={filter.protocol}
        onChange={(e) =>
          onChange({
            ...filter,
            protocol: e.target.value as ProtocolType | "all",
          })
        }
      >
        <option value="all">{t("transfer.filterAll")}</option>
        {PROTOCOL_TYPES.map((pt) => (
          <option key={pt} value={pt}>
            {pt.toUpperCase()}
          </option>
        ))}
      </select>

      <select
        style={selectStyle}
        value={filter.direction}
        onChange={(e) =>
          onChange({
            ...filter,
            direction: e.target.value as TransferDirection | "all",
          })
        }
      >
        <option value="all">{t("transfer.filterAll")}</option>
        <option value="send">{t("transfer.directionLabel_send")}</option>
        <option value="receive">
          {t("transfer.directionLabel_receive")}
        </option>
      </select>

      <select
        style={selectStyle}
        value={filter.status}
        onChange={(e) =>
          onChange({
            ...filter,
            status: e.target.value as TransferStatus | "all",
          })
        }
      >
        <option value="all">{t("transfer.filterAll")}</option>
        <option value="completed">{t("transfer.complete")}</option>
        <option value="failed">{t("transfer.failed")}</option>
        <option value="cancelled">{t("transfer.cancelled")}</option>
      </select>
    </div>
  );
}
