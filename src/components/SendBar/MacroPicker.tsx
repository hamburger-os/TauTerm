import { useTranslation } from "react-i18next";
import styles from "./AutoReplyRuleEditor.module.css";

interface MacroPickerProps {
  value: string;
  onChange: (value: string) => void;
}

/** 宏按钮信息 */
interface MacroDef {
  key: string;
  label: string;
  insert: string;
}

const MACROS: MacroDef[] = [
  { key: "macroCapture", label: "{{CAPTURE}}", insert: "{{CAPTURE(1)}}" },
  { key: "macroRandom", label: "{{RANDOM}}", insert: "{{RANDOM(0,100)}}" },
  { key: "macroRandomFloat", label: "{{RANDOM_F}}", insert: "{{RANDOM_F(0,100,1)}}" },
  { key: "macroTimestamp", label: "{{TIMESTAMP}}", insert: "{{TIMESTAMP}}" },
  { key: "macroDatetime", label: "{{DATETIME}}", insert: "{{DATETIME}}" },
  { key: "macroDatetimeFormat", label: "{{DATETIME_F}}", insert: "{{DATETIME_F(%H:%M:%S)}}" },
  { key: "macroCounter", label: "{{COUNTER}}", insert: "{{COUNTER}}" },
  { key: "macroHex", label: "{{HEX}}", insert: "{{HEX()}}" },
  { key: "macroHexval", label: "{{HEXVAL}}", insert: "{{HEXVAL(255,2)}}" },
  { key: "macroSin", label: "{{SIN}}", insert: "{{SIN(0,100,5000)}}" },
  { key: "macroExpr", label: "{{EXPR}}", insert: "{{EXPR:CAPTURE(1) * 2 + COUNTER}}" },
  { key: "macroCrc", label: "{{CRC}}", insert: "{{CRC(, 16, 0x8005)}}" },
  { key: "macroXorSum", label: "{{XOR_SUM}}", insert: "{{XOR_SUM()}}" },
  { key: "macroSum8", label: "{{SUM8}}", insert: "{{SUM8()}}" },
];

export default function MacroPicker({ value, onChange }: MacroPickerProps) {
  const { t } = useTranslation();

  const handleInsert = (insert: string) => {
    onChange(value + insert);
  };

  return (
    <div className={styles.macroPicker}>
      <span className={styles.macroLabel}>{t("sendBar.insertMacro")}:</span>
      <div className={styles.macroButtons}>
        {MACROS.map(m => (
          <button
            key={m.key}
            type="button"
            className={`${styles.macroBtn} liquid-glass-button`}
            onClick={() => handleInsert(m.insert)}
            title={t(`sendBar.${m.key}`)}
          >
            {m.label}
          </button>
        ))}
      </div>
    </div>
  );
}
