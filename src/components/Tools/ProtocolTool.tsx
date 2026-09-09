import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import RightSidebarPanel from "../RightSidebar/RightSidebarPanel";
import {
  DEFAULT_CUSTOM_SCHEMA,
  parseProtocolInput,
  PROTOCOL_TEMPLATE_INPUT_KIND,
  PROTOCOL_TEMPLATE_NAMES,
  type ParsedField,
  type ProtocolDirection,
  type ProtocolRange,
  type ProtocolTemplate,
} from "../../utils/protocolParsing";
import styles from "./ProtocolTool.module.css";

const PLACEHOLDER_KEYS: Record<ProtocolTemplate, string> = {
  auto: "tools.protocolPlaceholderAuto",
  "modbus-rtu": "tools.protocolPlaceholderModbusRTU",
  "modbus-ascii": "tools.protocolPlaceholderModbusASCII",
  "modbus-tcp": "tools.protocolPlaceholderModbusTCP",
  "at-response": "tools.protocolPlaceholderAT",
  "nmea-0183": "tools.protocolPlaceholderNmea",
  raw: "tools.protocolPlaceholderRaw",
  "custom-schema": "tools.protocolPlaceholderCustomSchema",
};

interface FlatField {
  field: ParsedField;
  depth: number;
}

function flattenFields(fields: ParsedField[], depth = 0): FlatField[] {
  const output: FlatField[] = [];
  for (const field of fields) {
    output.push({ field, depth });
    if (field.children) output.push(...flattenFields(field.children, depth + 1));
  }
  return output;
}

function rangeText(range: ProtocolRange): string {
  const unit = range.unit === "byte" ? "B" : "ch";
  return String(range.start) + " +" + String(range.length) + " " + unit;
}

function isWithin(index: number, range: ProtocolRange | null): boolean {
  return Boolean(
    range
    && index >= range.start
    && index < range.start + range.length,
  );
}

export interface ProtocolToolProps {\n  sessionId: string;\n}\n\nexport default function ProtocolTool({ sessionId }: ProtocolToolProps) {
  const { t } = useTranslation();
  const [template, setTemplate] = useState<ProtocolTemplate>("auto");
  const [input, setInput] = useState("");
  const [direction, setDirection] = useState<ProtocolDirection>("auto");
  const [customSchema, setCustomSchema] = useState(DEFAULT_CUSTOM_SCHEMA);
  const [highlightedRange, setHighlightedRange] = useState<ProtocolRange | null>(null);


  useEffect(() => {
    const handler = (event: Event) => {
      const detail = (event as CustomEvent<{ sessionId?: string; input?: string }>).detail;
      if (detail?.sessionId !== sessionId || typeof detail.input !== "string") return;
      setTemplate("auto");
      setDirection("auto");
      setInput(detail.input);
      setHighlightedRange(null);
    };
    window.addEventListener("tauterm:protocol-inspect", handler);
    return () => window.removeEventListener("tauterm:protocol-inspect", handler);
  }, [sessionId]);

  const outcome = useMemo(
    () => parseProtocolInput(template, input, { direction, customSchema }),
    [customSchema, direction, input, template],
  );
  const inputKind = PROTOCOL_TEMPLATE_INPUT_KIND[template];
  const flatFields = useMemo(
    () => outcome.result ? flattenFields(outcome.result.fields) : [],
    [outcome.result],
  );
  const activeInspector = outcome.detectedTemplate ?? outcome.result?.inspectorId;
  const showDirection =
    template.startsWith("modbus-")
    || activeInspector?.startsWith("modbus-");

  return (
    <RightSidebarPanel title={t("tools.protocolInspector")}>
      <div className={styles.container}>
        <select
          className={styles.select + " liquid-glass-input liquid-glass-select"}
          value={template}
          onChange={(event) => {
            setTemplate(event.target.value as ProtocolTemplate);
            setHighlightedRange(null);
          }}
          aria-label={t("tools.protocolTemplate")}
        >
          {Object.entries(PROTOCOL_TEMPLATE_NAMES).map(([key, labelKey]) => (
            <option key={key} value={key}>{t(labelKey)}</option>
          ))}
        </select>

        {showDirection && (
          <div className={styles.directionRow}>
            <span>{t("tools.protocolDirection")}</span>
            <div className="liquid-selector-strip">
              {(["auto", "request", "response"] as ProtocolDirection[]).map((item) => (
                <button
                  key={item}
                  type="button"
                  className={"liquid-glass-button liquid-selector-button " + (direction === item ? "active" : "")}
                  onClick={() => setDirection(item)}
                >
                  {t("tools.protocolDirections." + item)}
                </button>
              ))}
            </div>
          </div>
        )}

        {template === "custom-schema" && (
          <details className={styles.schemaDetails}>
            <summary>{t("tools.customSchema")}</summary>
            <textarea
              className={styles.schemaInput + " liquid-glass-input liquid-glass-textarea"}
              value={customSchema}
              onChange={(event) => setCustomSchema(event.target.value)}
              rows={9}
              spellCheck={false}
            />
          </details>
        )}

        <div className={styles.inputHeader}>
          <span>{t("tools.protocolInput")}</span>
          <span className={styles.inputKind}>
            {inputKind === "hex"
              ? "HEX"
              : inputKind === "text"
                ? t("tools.textInput")
                : t("tools.autoInput")}
          </span>
        </div>

        <textarea
          className={styles.input + " liquid-glass-input liquid-glass-textarea"}
          value={input}
          onChange={(event) => setInput(event.target.value)}
          placeholder={t(PLACEHOLDER_KEYS[template])}
          rows={4}
          spellCheck={false}
        />

        {outcome.result && (
          <div className={styles.resultSection}>
            <div className={styles.inspectorHeader}>
              <strong>{t(PROTOCOL_TEMPLATE_NAMES[outcome.result.inspectorId])}</strong>
              {outcome.detectedTemplate && (
                <span className={styles.detectBadge}>
                  {t("tools.detectedProtocol")}
                  {" · "}
                  {t("tools.confidence." + (outcome.result.detectedConfidence ?? "low"))}
                </span>
              )}
              {outcome.result.effectiveDirection && (
                <span className={styles.directionBadge}>
                  {t("tools.protocolDirections." + outcome.result.effectiveDirection)}
                </span>
              )}
            </div>

            <div className={styles.rawBlock}>
              <div className={styles.rawHeader}>{t("tools.rawFrame")}</div>
              {outcome.result.rawBytes ? (
                <code className={styles.rawBytes}>
                  {Array.from(outcome.result.rawBytes).map((byte, index) => (
                    <span
                      key={index}
                      className={
                        highlightedRange?.unit === "byte"
                        && isWithin(index, highlightedRange)
                          ? styles.rawHighlight
                          : ""
                      }
                    >
                      {byte.toString(16).toUpperCase().padStart(2, "0")}
                      {index < outcome.result!.rawBytes!.length - 1 ? " " : ""}
                    </span>
                  ))}
                </code>
              ) : (
                <code className={styles.rawText}>
                  {Array.from(outcome.result.normalizedInput).map((char, index) => (
                    <span
                      key={index}
                      className={
                        highlightedRange?.unit === "char"
                        && isWithin(index, highlightedRange)
                          ? styles.rawHighlight
                          : ""
                      }
                    >
                      {char}
                    </span>
                  ))}
                </code>
              )}
            </div>

            <div className={styles.checkGrid}>
              {outcome.result.checks.map((check) => (
                <div
                  key={check.id}
                  className={styles.checkItem + " " + styles["check_" + check.status]}
                  title={check.detail}
                >
                  <span>
                    {check.status === "pass"
                      ? "✓"
                      : check.status === "fail"
                        ? "✕"
                        : check.status === "warning"
                          ? "!"
                          : "—"}
                  </span>
                  <span>{check.label.startsWith("tools.") ? t(check.label) : check.label}</span>
                </div>
              ))}
            </div>

            {flatFields.length > 0 && (
              <div className={styles.tableWrap}>
                <table className={styles.table}>
                  <thead>
                    <tr>
                      <th>{t("tools.protocolField")}</th>
                      <th>{t("tools.protocolRange")}</th>
                      <th>{t("tools.protocolValue")}</th>
                    </tr>
                  </thead>
                  <tbody>
                    {flatFields.map(({ field, depth }) => (
                      <tr
                        key={field.id + "-" + field.range.start + "-" + depth}
                        onMouseEnter={() => setHighlightedRange(field.range)}
                        onMouseLeave={() => setHighlightedRange(null)}
                      >
                        <td>
                          <span
                            className={styles.fieldName}
                            style={{ paddingInlineStart: depth * 10 }}
                          >
                            {depth > 0 && <span className={styles.treeMark}>↳</span>}
                            {field.name.startsWith("tools.")
                              ? t(field.name, field.nameParams ?? {})
                              : field.name}
                          </span>
                        </td>
                        <td><code>{rangeText(field.range)}</code></td>
                        <td className={styles.fieldValue}>
                          {field.rawValue && (
                            <code className={styles.hexCode}>{field.rawValue}</code>
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

            {outcome.result.issues.length > 0 && (
              <div className={styles.issueList}>
                {outcome.result.issues.map((issue, index) => (
                  <div
                    key={issue.code + "-" + index}
                    className={styles.issue + " " + styles["issue_" + issue.severity]}
                    onMouseEnter={() => setHighlightedRange(issue.range ?? null)}
                    onMouseLeave={() => setHighlightedRange(null)}
                  >
                    <span>
                      {issue.severity === "error" ? "✕" : issue.severity === "warning" ? "!" : "i"}
                    </span>
                    <span>
                      {t("tools.protocolIssues." + issue.code, {
                        detail: issue.detail ?? "",
                        defaultValue: issue.detail
                          ? issue.code + ": " + issue.detail
                          : issue.code,
                      })}
                    </span>
                  </div>
                ))}
              </div>
            )}
          </div>
        )}

        {!outcome.result && !input.trim() && (
          <div className={styles.placeholder}>
            {t(
              inputKind === "hex"
                ? "tools.protocolHintHex"
                : inputKind === "text"
                  ? "tools.protocolHintText"
                  : "tools.protocolHintAuto",
            )}
          </div>
        )}

        {!outcome.result && input.trim() && outcome.errorCode && (
          <div className={styles.parseError}>
            {t("tools.errors." + outcome.errorCode, {
              defaultValue: outcome.errorCode,
            })}
          </div>
        )}
      </div>
    </RightSidebarPanel>
  );
}
