import { useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import RightSidebarPanel from "../RightSidebar/RightSidebarPanel";
import {
  parseProtocolInput,
  PROTOCOL_TEMPLATE_INPUT_KIND,
  PROTOCOL_TEMPLATE_NAMES,
  type ProtocolTemplate,
} from "../../utils/protocolParsing";
import styles from "./ProtocolTool.module.css";

const PLACEHOLDER_KEYS: Record<ProtocolTemplate, string> = {
  "modbus-rtu": "tools.protocolPlaceholderModbusRTU",
  "modbus-ascii": "tools.protocolPlaceholderModbusASCII",
  "at-response": "tools.protocolPlaceholderAT",
  "custom": "tools.protocolPlaceholderCustom",
};

export default function ProtocolTool() {
  const { t } = useTranslation();

  const [template, setTemplate] = useState<ProtocolTemplate>("modbus-rtu");
  const [input, setInput] = useState("");

  const outcome = useMemo(
    () => parseProtocolInput(template, input),
    [input, template],
  );
  const inputKind = PROTOCOL_TEMPLATE_INPUT_KIND[template];

  return (
    <RightSidebarPanel title={t("tools.protocol") ?? "Protocol Parser"}>
      <div className={styles.container}>
        <select
          className={`${styles.select} liquid-glass-input liquid-glass-select`}
          value={template}
          onChange={(e) => setTemplate(e.target.value as ProtocolTemplate)}
          aria-label={t("tools.protocolTemplate") ?? "Protocol template"}
        >
          {Object.entries(PROTOCOL_TEMPLATE_NAMES).map(([key, labelKey]) => (
            <option key={key} value={key}>{t(labelKey)}</option>
          ))}
        </select>

        <div className={styles.inputHeader}>
          <span>{t("tools.protocolInput") ?? "Input"}</span>
          <span className={styles.inputKind}>
            {inputKind === "hex" ? "HEX" : t("tools.textInput") ?? "TEXT"}
          </span>
        </div>

        <textarea
          className={`${styles.input} liquid-glass-input liquid-glass-textarea`}
          value={input}
          onChange={(e) => setInput(e.target.value)}
          placeholder={t(PLACEHOLDER_KEYS[template])}
          rows={3}
          spellCheck={false}
        />

        {outcome.result && (
          <div className={styles.resultSection}>
            {outcome.result.fields.length > 0 && (
              <div className={styles.tableWrap}>
                <table className={styles.table}>
                  <thead>
                    <tr>
                      <th>{t("tools.protocolField") ?? "Field"}</th>
                      <th>{t("tools.protocolOffset") ?? "Offset"}</th>
                      <th>{t("tools.protocolValue") ?? "Value"}</th>
                    </tr>
                  </thead>
                  <tbody>
                    {outcome.result.fields.map((field, index) => (
                      <tr key={index}>
                        <td>
                          <code>
                            {field.name.startsWith("tools.")
                              ? t(field.name, field.nameParams ?? {})
                              : field.name}
                          </code>
                        </td>
                        <td>{field.offset}</td>
                        <td className={styles.fieldValue}>
                          {field.hexValue && (
                            <code className={styles.hexCode}>{field.hexValue}</code>
                          )}
                          {field.parsedValue && (
                            <span className={styles.parsedVal}>
                              {field.parsedValue.startsWith("tools.")
                                ? t(field.parsedValue)
                                : field.parsedValue}
                            </span>
                          )}
                        </td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
            )}

            {outcome.result.checksumValid !== null && (
              <div
                className={`${styles.checksumResult} ${
                  outcome.result.checksumValid ? styles.valid : styles.invalid
                }`}
              >
                {outcome.result.checksumValid
                  ? (outcome.result.checksumType === "lrc"
                    ? t("tools.lrcValid")
                    : t("tools.crcValid"))
                  : (outcome.result.checksumType === "lrc"
                    ? t("tools.lrcMismatch", {
                      expected: outcome.result.checksumExpected ?? "",
                    })
                    : t("tools.crcMismatch", {
                      expected: outcome.result.checksumExpected ?? "",
                    }))}
              </div>
            )}

            {outcome.result.checksumValid === null
              && outcome.result.checksumInfo && (
              <div className={styles.checksumInfo}>
                {t(outcome.result.checksumInfo)}
              </div>
            )}
          </div>
        )}

        {!outcome.result && !input.trim() && (
          <div className={styles.placeholder}>
            {t(
              inputKind === "hex"
                ? "tools.protocolHintHex"
                : "tools.protocolHintText",
            )}
          </div>
        )}

        {!outcome.result && input.trim() && outcome.errorKey && (
          <div className={styles.parseError}>
            {t(outcome.errorKey)}
          </div>
        )}
      </div>
    </RightSidebarPanel>
  );
}
