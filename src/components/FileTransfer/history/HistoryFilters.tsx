import { useTranslation } from "react-i18next";
import type {
  HistoryFilter,
  ProtocolType,
  TransferDirection,
  TransferStatus,
} from "../../../types/transfer";
import { PROTOCOL_TYPES } from "../../../types/transfer";
import inputStyles from "../../common/GlassInput.module.css";

interface HistoryFiltersProps {
  filter: HistoryFilter;
  onChange: (f: HistoryFilter) => void;
}

export default function HistoryFilters({
  filter,
  onChange,
}: HistoryFiltersProps) {
  const { t } = useTranslation();

  const compactStyle: React.CSSProperties = {
    fontSize: "0.6rem",
    padding: "2px 4px",
  };

  return (
    <div
      style={{
        display: "flex",
        gap: "4px",
        flexShrink: 0,
      }}
    >
      <select
        className={`${inputStyles.input} ${inputStyles.select}`}
        style={compactStyle}
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
        className={`${inputStyles.input} ${inputStyles.select}`}
        style={compactStyle}
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
        className={`${inputStyles.input} ${inputStyles.select}`}
        style={compactStyle}
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
