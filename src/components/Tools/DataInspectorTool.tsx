import { useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { inspectByteData } from "../../utils/dataInspector";
import { copyToClipboard } from "../../utils/clipboard";
import styles from "./DataInspectorTool.module.css";

export default function DataInspectorTool() {
  const { t } = useTranslation();
  const [input, setInput] = useState("");
  const [copiedId, setCopiedId] = useState<string | null>(null);

  const outcome = useMemo(
    () => input.trim() ? inspectByteData(input) : null,
    [input],
  );

  const copyValue = async (id: string, value: string) => {
    await copyToClipboard(value);
    setCopiedId(id);
    window.setTimeout(() => setCopiedId(null), 1200);
  };

  return (
    <div className={styles.container}>
      <div className={styles.hint}>{t("tools.byteInputFormatsHint")}</div>
      <textarea
        className={styles.input + " liquid-glass-input liquid-glass-textarea"}
        value={input}
        onChange={(event) => setInput(event.target.value)}
        placeholder={t("tools.dataInspectorPlaceholder")}
        rows={3}
        spellCheck={false}
      />

      {outcome?.ok && (
        <>
          <code className={styles.normalized}>{outcome.value.normalizedHex}</code>
          <div className={styles.tableWrap}>
            <table className={styles.table}>
              <tbody>
                {outcome.value.interpretations.map((item) => (
                  <tr key={item.id}>
                    <th>
                      {item.label.startsWith("tools.") ? t(item.label) : item.label}
                    </th>
                    <td><code>{item.value}</code></td>
                    <td>
                      <button
                        className="liquid-glass-ghost-button"
                        type="button"
                        onClick={() => copyValue(item.id, item.value)}
                        title={t("common.copy")}
                      >
                        {copiedId === item.id ? t("tools.copied") : t("common.copy")}
                      </button>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </>
      )}

      {outcome && !outcome.ok && (
        <div className={styles.error}>
          {t("tools.errors." + outcome.error.code, {
            detail: outcome.error.detail ?? "",
            defaultValue: outcome.error.code,
          })}
        </div>
      )}

      {!input.trim() && (
        <div className={styles.placeholder}>{t("tools.dataInspectorHint")}</div>
      )}
    </div>
  );
}
