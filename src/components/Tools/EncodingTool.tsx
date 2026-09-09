import { useCallback, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import RightSidebarPanel from "../RightSidebar/RightSidebarPanel";
import {
  ENCODING_OP_KEYS,
  executeEncodingOp,
  type EncodingOp,
} from "../../utils/encoding";
import { copyToClipboard } from "../../utils/clipboard";
import ToolInputHistory from "./ToolInputHistory";
import { useToolInputHistory } from "../../hooks/useToolInputHistory";
import styles from "./EncodingTool.module.css";

function encodingHint(operation: EncodingOp): string | null {
  if (
    operation === "string-to-hex"
    || operation === "base64-encode"
  ) return "tools.utf8InputHint";
  if (operation === "hex-to-string") return "tools.strictUtf8OutputHint";
  return null;
}

export function EncodingToolInner() {
  const { t } = useTranslation();
  const [inputText, setInputText] = useState("");
  const [operation, setOperation] = useState<EncodingOp>("string-to-hex");
  const [base64IgnoreWhitespace, setBase64IgnoreWhitespace] = useState(false);
  const [copied, setCopied] = useState(false);

  const outcome = useMemo(() => {
    if (!inputText.trim()) return null;
    return executeEncodingOp(inputText, operation, { base64IgnoreWhitespace });
  }, [base64IgnoreWhitespace, inputText, operation]);

  const handleCopy = useCallback(async () => {
    if (!outcome?.ok) return;
    await copyToClipboard(outcome.value);
    setCopied(true);
    window.setTimeout(() => setCopied(false), 1500);
  }, [outcome]);

  const history = useToolInputHistory(inputText, Boolean(outcome?.ok));
  const hint = encodingHint(operation);

  return (
    <div className={styles.container}>
      <select
        className={styles.select + " liquid-glass-input liquid-glass-select"}
        value={operation}
        onChange={(event) => setOperation(event.target.value as EncodingOp)}
        aria-label={t("tools.encodingOperation")}
      >
        {ENCODING_OP_KEYS.map((key) => (
          <option key={key} value={key}>
            {t("tools.encodingOps." + key)}
          </option>
        ))}
      </select>

      {operation === "base64-decode" && (
        <div className={styles.optionRow}>
          <label className={styles.checkboxLabel}>
            <input
              type="checkbox"
              checked={base64IgnoreWhitespace}
              onChange={(event) => setBase64IgnoreWhitespace(event.target.checked)}
            />
            {t("tools.base64IgnoreWhitespace")}
          </label>
        </div>
      )}

      {hint && <div className={styles.hint}>{t(hint)}</div>}

      <textarea
        className={styles.input + " liquid-glass-input liquid-glass-textarea"}
        value={inputText}
        onChange={(event) => setInputText(event.target.value)}
        placeholder={t("tools.encodingInputPlaceholder")}
        rows={3}
        spellCheck={false}
      />

      <ToolInputHistory
        entries={history.entries}
        onSelect={setInputText}
        onTogglePinned={history.togglePinned}
        onClearRecent={history.clearRecent}
      />

      {outcome?.ok && (
        <div className={styles.resultRow}>
          <code className={styles.resultCode}>{outcome.value}</code>
          <button
            className={styles.copyBtn + " liquid-glass-ghost-button"}
            onClick={handleCopy}
            type="button"
            title={t("common.copy")}
          >
            {copied ? t("tools.copied") : t("common.copy")}
          </button>
        </div>
      )}

      {outcome && !outcome.ok && (
        <div className={styles.resultErrorCode}>
          {t("tools.errors." + outcome.error.code, {
            detail: outcome.error.detail ?? "",
            position: outcome.error.position ?? "",
            defaultValue:
              outcome.error.detail
                ? outcome.error.code + ": " + outcome.error.detail
                : outcome.error.code,
          })}
        </div>
      )}

      {!inputText.trim() && (
        <div className={styles.placeholder}>
          {t("tools.encodingHint")}
        </div>
      )}
    </div>
  );
}

export default function EncodingTool() {
  const { t } = useTranslation();
  return (
    <RightSidebarPanel title={t("tools.encoding")}>
      <EncodingToolInner />
    </RightSidebarPanel>
  );
}
