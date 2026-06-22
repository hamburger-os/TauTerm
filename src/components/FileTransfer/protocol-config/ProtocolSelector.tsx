import { useTranslation } from "react-i18next";
import type { ProtocolType, TransferConfig } from "../../../types/transfer";
import { PROTOCOL_TYPES, PROTOCOL_REGISTRY } from "../../../types/transfer";
import inputStyles from "../../common/GlassInput.module.css";

interface ProtocolSelectorProps {
  value: TransferConfig;
  onChange: (config: TransferConfig) => void;
}

/** 协议下拉选择器 + 描述 */
export default function ProtocolSelector({
  value,
  onChange,
}: ProtocolSelectorProps) {
  const { t } = useTranslation();

  const handleChange = (e: React.ChangeEvent<HTMLSelectElement>) => {
    const newProtocol = e.target.value as ProtocolType;
    const newConfig = PROTOCOL_REGISTRY[newProtocol].defaultConfig;
    onChange(newConfig);
  };

  const meta = PROTOCOL_REGISTRY[value.protocol];

  return (
    <div style={{ display: "flex", flexDirection: "column", gap: "4px" }}>
      <select
        value={value.protocol}
        onChange={handleChange}
        className={`${inputStyles.input} ${inputStyles.select} liquid-glass-input`}
        style={{
          fontSize: "var(--text-xs)",
        }}
      >
        {PROTOCOL_TYPES.map((pt) => (
          <option key={pt} value={pt}>
            {PROTOCOL_REGISTRY[pt].icon}{" "}
            {t(PROTOCOL_REGISTRY[pt].i18nKey)}
          </option>
        ))}
      </select>
      <span
        style={{
          fontSize: "0.6rem",
          color: "var(--text-muted)",
          lineHeight: "1.3",
        }}
      >
        {t(meta.i18nKey + "Description" as any, "") ||
          (t(meta.i18nKey) || value.protocol.toUpperCase())}
      </span>
    </div>
  );
}
