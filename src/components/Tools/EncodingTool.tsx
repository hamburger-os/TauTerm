import { useState, useCallback, useMemo } from "react";
import { useTranslation } from "react-i18next";
import RightSidebarPanel from "../RightSidebar/RightSidebarPanel";
import {
  executeEncodingOp,
  type EncodingOp,
  ENCODING_OP_KEYS,
} from "../../utils/encoding";
import styles from "./EncodingTool.module.css";

export function EncodingToolInner() {
  const { t } = useTranslation();

  const [inputText, setInputText] = useState("");
  const [operation, setOperation] = useState<EncodingOp>("string-to-hex");
  const [copied, setCopied] = useState(false);

  const result = useMemo(() => {
    if (!inputText.trim()) return "";
    return executeEncodingOp(inputText, operation);
  }, [inputText, operation]);

  const handleCopy = useCallback(async () => {
    if (!result) return;
    try {
      await navigator.clipboard.writeText(result);
      setCopied(true);
      setTimeout(() => setCopied(false), 1500);
    } catch { /* ignore */ }
  }, [result]);

  return (
    <div className={styles.container}>
      {/* 转换操作选择 */}
      <select
        className={`${styles.select} liquid-glass-input liquid-glass-select`}
        value={operation}
        onChange={(e) => setOperation(e.target.value as EncodingOp)}
      >
        {ENCODING_OP_KEYS.map((key) => (
          <option key={key} value={key}>{t(`tools.encodingOps.${key}`)}</option>
        ))}
      </select>

      {/* 输入 */}
      <textarea
        className={`${styles.input} liquid-glass-input liquid-glass-textarea`}
        value={inputText}
        onChange={(e) => setInputText(e.target.value)}
        placeholder={t("tools.encodingInputPlaceholder") ?? "Enter data to convert..."}
        rows={3}
        spellCheck={false}
      />

      {/* 结果 */}
      {result && (
        <div className={`${styles.resultRow} ${result.startsWith("[Error:") ? styles.resultError : ""}`}>
          <code className={result.startsWith("[Error:") ? styles.resultErrorCode : styles.resultCode}>
            {result}
          </code>
          {!result.startsWith("[Error:") && (
            <button className={styles.copyBtn} onClick={handleCopy} title={t("common.copy") ?? "Copy"}>
              {copied ? t("tools.copied") : t("common.copy")}
            </button>
          )}
        </div>
      )}

      {!inputText.trim() && (
        <div className={styles.placeholder}>
          {t("tools.encodingHint") ?? "Select conversion and enter data"}
        </div>
      )}
    </div>
  );
}

export default function EncodingTool() {
  const { t } = useTranslation();
  return (
    <RightSidebarPanel title={t("tools.encoding") ?? "Encoding Conv"}>
      <EncodingToolInner />
    </RightSidebarPanel>
  );
}
