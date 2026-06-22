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
    <div className={inputStyles.wrapper}>
      <select
        value={value.protocol}
        onChange={handleChange}
        className={`${inputStyles.input} ${inputStyles.select} ${inputStyles.selectSmall} liquid-glass-input`}
      >
        {PROTOCOL_TYPES.map((pt) => (
          <option key={pt} value={pt}>
            {PROTOCOL_REGISTRY[pt].icon}{" "}
            {t(PROTOCOL_REGISTRY[pt].i18nKey)}
          </option>
        ))}
      </select>
      <span className={inputStyles.description}>
        {t(meta.i18nKey + "Description" as any, "") ||
          (t(meta.i18nKey) || value.protocol.toUpperCase())}
      </span>
    </div>
  );
}
