import { useTranslation } from "react-i18next";
import type { ProtocolType, TransferConfig } from "../../../types/transfer";
import { PROTOCOL_TYPES, PROTOCOL_REGISTRY } from "../../../types/transfer";
import inputStyles from "../../common/GlassInput.module.css";

interface ProtocolSelectorProps {
  value: TransferConfig;
  onChange: (config: TransferConfig) => void;
}

/** 协议下拉选择器 */
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

  return (
    <div className={inputStyles.wrapper}>
      <select
        value={value.protocol}
        onChange={handleChange}
        className={`${inputStyles.input} ${inputStyles.select} ${inputStyles.selectSmall} liquid-glass-input liquid-glass-select`}
      >
        {PROTOCOL_TYPES.map((pt) => (
          <option key={pt} value={pt}>
            {t(PROTOCOL_REGISTRY[pt].i18nKey)}
          </option>
        ))}
      </select>
    </div>
  );
}
