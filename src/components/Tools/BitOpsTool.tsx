import { useCallback, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import RightSidebarPanel from "../RightSidebar/RightSidebarPanel";
import {
  OP_KEYS,
  bitwiseOp,
  extractBitRange,
  parseIntegerInput,
  parseStructDefinition,
  toggleBit,
  type BitOp,
  type BitWidth,
  type StructAbi,
  type StructPack,
} from "../../utils/bitops";
import styles from "./BitOpsTool.module.css";

type ToolMode = "bitwise" | "sizeof";

const WIDTHS: BitWidth[] = [8, 16, 32, 64];
const ABIS: StructAbi[] = ["ILP32", "LP64", "LLP64"];
const PACKS: StructPack[] = [0, 1, 2, 4, 8];

export function BitOpsToolInner() {
  const { t } = useTranslation();
  const [mode, setMode] = useState<ToolMode>("bitwise");
  const [width, setWidth] = useState<BitWidth>(32);
  const [opA, setOpA] = useState("");
  const [opB, setOpB] = useState("");
  const [bitOp, setBitOp] = useState<BitOp>("AND");
  const [rangeHigh, setRangeHigh] = useState("7");
  const [rangeLow, setRangeLow] = useState("0");
  const [structCode, setStructCode] = useState("");
  const [abi, setAbi] = useState<StructAbi>("ILP32");
  const [pack, setPack] = useState<StructPack>(0);

  const parsedA = useMemo(() => parseIntegerInput(opA, width), [opA, width]);
  const parsedB = useMemo(() => parseIntegerInput(opB, width), [opB, width]);

  const bitwiseInputError = useMemo(() => {
    if (!opA.trim() && !opB.trim()) return null;
    if (opA.trim() && parsedA === null) return "tools.invalidNumber";
    if (bitOp !== "NOT" && opB.trim() && parsedB === null) return "tools.invalidNumber";
    if (
      (bitOp === "LSHIFT" || bitOp === "RSHIFT" || bitOp === "URSHIFT")
      && parsedB !== null
      && (parsedB < 0n || parsedB >= BigInt(width))
    ) return "tools.shiftCountRangeWidth";
    return null;
  }, [bitOp, opA, opB, parsedA, parsedB, width]);

  const bitResult = useMemo(() => {
    if (parsedA === null) return null;
    if (bitOp !== "NOT" && parsedB === null) return null;
    if (
      (bitOp === "LSHIFT" || bitOp === "RSHIFT" || bitOp === "URSHIFT")
      && (parsedB === null || parsedB < 0n || parsedB >= BigInt(width))
    ) return null;
    return bitwiseOp(parsedA, parsedB ?? 0n, bitOp, width);
  }, [bitOp, parsedA, parsedB, width]);

  const rangeResult = useMemo(() => {
    if (parsedA === null) return null;
    const high = Number(rangeHigh);
    const low = Number(rangeLow);
    if (!Number.isInteger(high) || !Number.isInteger(low)) return null;
    return extractBitRange(parsedA, high, low, width);
  }, [parsedA, rangeHigh, rangeLow, width]);

  const structResult = useMemo(() => {
    if (!structCode.trim()) return null;
    return parseStructDefinition(structCode, abi, pack);
  }, [abi, pack, structCode]);

  const handleToggleBit = useCallback((bit: number) => {
    const currentValue = parseIntegerInput(opA || "0", width) ?? 0n;
    const next = toggleBit(currentValue, bit, width);
    setOpA("0x" + next.toString(16).toUpperCase().padStart(width / 4, "0"));
  }, [opA, width]);

  return (
    <div className={styles.container}>
      <div className={styles.modeRow + " liquid-selector-strip"}>
        <button
          className={styles.modeBtn + " liquid-glass-button liquid-selector-button " + (mode === "bitwise" ? "active" : "")}
          onClick={() => setMode("bitwise")}
          type="button"
          aria-pressed={mode === "bitwise"}
        >
          {t("tools.bitwiseMode")}
        </button>
        <button
          className={styles.modeBtn + " liquid-glass-button liquid-selector-button " + (mode === "sizeof" ? "active" : "")}
          onClick={() => setMode("sizeof")}
          type="button"
          aria-pressed={mode === "sizeof"}
        >
          {t("tools.structLayoutMode")}
        </button>
      </div>

      {mode === "bitwise" && (
        <div className={styles.bitwiseSection}>
          <div className={styles.widthRow}>
            <span className={styles.label}>{t("tools.bitWidth")}:</span>
            <div className="liquid-selector-strip">
              {WIDTHS.map((item) => (
                <button
                  key={item}
                  className={"liquid-glass-button liquid-selector-button " + (width === item ? "active" : "")}
                  onClick={() => setWidth(item)}
                  type="button"
                >
                  {item}
                </button>
              ))}
            </div>
          </div>

          <div className={styles.opRow}>
            <label className={styles.label}>A:</label>
            <input
              className={styles.opInput + " liquid-glass-input"}
              value={opA}
              onChange={(event) => setOpA(event.target.value)}
              placeholder={t("tools.bitwiseOperandPlaceholder")}
              spellCheck={false}
            />
          </div>

          <div className={styles.opRow}>
            <label className={styles.label}>{t("tools.operator")}:</label>
            <select
              className={styles.select + " liquid-glass-input liquid-glass-select"}
              value={bitOp}
              onChange={(event) => setBitOp(event.target.value as BitOp)}
            >
              {OP_KEYS.map((key) => (
                <option key={key} value={key}>{t("tools.bitOps." + key)}</option>
              ))}
            </select>
          </div>

          {bitOp !== "NOT" && (
            <div className={styles.opRow}>
              <label className={styles.label}>B:</label>
              <input
                className={styles.opInput + " liquid-glass-input"}
                value={opB}
                onChange={(event) => setOpB(event.target.value)}
                placeholder={t("tools.bitwiseOperandPlaceholder")}
                spellCheck={false}
              />
            </div>
          )}

          {bitwiseInputError && (
            <div className={styles.parseError}>
              {t(bitwiseInputError, { width })}
            </div>
          )}

          {bitResult && (
            <div className={styles.bitResult}>
              <div className={styles.resultHeader}>
                <span>{t("tools.result")}:</span>
                <code className={styles.resultVal}>0x{bitResult.hex}</code>
              </div>
              <div className={styles.numericGrid}>
                <span>{t("tools.unsignedValue")}</span>
                <code>{bitResult.unsigned}</code>
                <span>{t("tools.signedValue")}</span>
                <code>{bitResult.signed}</code>
              </div>
              <div className={styles.bitsDisplay} aria-label={t("tools.bitEditor")}>
                {Array.from({ length: width }, (_, index) => width - 1 - index).map((bit) => {
                  const active = ((BigInt("0x" + bitResult.hex) >> BigInt(bit)) & 1n) === 1n;
                  return (
                    <button
                      key={bit}
                      type="button"
                      className={styles.bitCell + " " + (active ? styles.bitCellActive : "")}
                      onClick={() => handleToggleBit(bit)}
                      title={"bit " + bit}
                    >
                      <span>{bit}</span>
                      <strong>{active ? "1" : "0"}</strong>
                    </button>
                  );
                })}
              </div>

              <div className={styles.rangeRow}>
                <span>{t("tools.extractBits")}</span>
                <input
                  className={styles.rangeInput + " liquid-glass-input"}
                  inputMode="numeric"
                  value={rangeHigh}
                  onChange={(event) => setRangeHigh(event.target.value)}
                  aria-label={t("tools.highBit")}
                />
                <span>:</span>
                <input
                  className={styles.rangeInput + " liquid-glass-input"}
                  inputMode="numeric"
                  value={rangeLow}
                  onChange={(event) => setRangeLow(event.target.value)}
                  aria-label={t("tools.lowBit")}
                />
              </div>
              {rangeResult && (
                <div className={styles.numericGrid}>
                  <span>{t("tools.mask")}</span>
                  <code>0x{rangeResult.maskHex}</code>
                  <span>{t("tools.extractedValue")}</span>
                  <code>0x{rangeResult.valueHex} ({rangeResult.unsigned})</code>
                </div>
              )}
            </div>
          )}
        </div>
      )}

      {mode === "sizeof" && (
        <div className={styles.sizeofSection}>
          <div className={styles.toolHint}>{t("tools.structAbiHint")}</div>
          <div className={styles.structOptions}>
            <label>
              ABI
              <select
                className="liquid-glass-input liquid-glass-select"
                value={abi}
                onChange={(event) => setAbi(event.target.value as StructAbi)}
              >
                {ABIS.map((item) => <option key={item} value={item}>{item}</option>)}
              </select>
            </label>
            <label>
              Packing
              <select
                className="liquid-glass-input liquid-glass-select"
                value={pack}
                onChange={(event) => setPack(Number(event.target.value) as StructPack)}
              >
                {PACKS.map((item) => (
                  <option key={item} value={item}>
                    {item === 0 ? t("tools.defaultPacking") : item}
                  </option>
                ))}
              </select>
            </label>
          </div>
          <textarea
            className={styles.structInput + " liquid-glass-input liquid-glass-textarea"}
            value={structCode}
            onChange={(event) => setStructCode(event.target.value)}
            placeholder={"struct {\n  char a;\n  int b;\n  char c;\n}"}
            rows={6}
            spellCheck={false}
          />

          {structResult && (
            <div className={styles.structResult}>
              <table className={styles.table}>
                <thead>
                  <tr>
                    <th>{t("tools.structMember")}</th>
                    <th>{t("tools.structType")}</th>
                    <th>{t("tools.structOffset")}</th>
                    <th>{t("tools.structSize")}</th>
                    <th>{t("tools.structPadBefore")}</th>
                  </tr>
                </thead>
                <tbody>
                  {structResult.members.map((member) => (
                    <tr key={member.name}>
                      <td><code>{member.name}</code></td>
                      <td><code>{member.type}</code></td>
                      <td>{member.offset}</td>
                      <td>{member.size}</td>
                      <td className={member.paddingBefore > 0 ? styles.padHighlight : ""}>
                        {member.paddingBefore > 0 ? member.paddingBefore : "-"}
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
              <div className={styles.totalRow}>
                sizeof = {structResult.totalSize}
                <span className={styles.alignNote}>
                  align {structResult.alignment}
                  {" · "}
                  {t("tools.tailPadding")} {structResult.tailPadding}
                </span>
              </div>
            </div>
          )}

          {!structResult && structCode.trim() && (
            <div className={styles.parseError}>{t("tools.structParseError")}</div>
          )}
        </div>
      )}
    </div>
  );
}

export default function BitOpsTool() {
  const { t } = useTranslation();
  return (
    <RightSidebarPanel title={t("tools.bitops")}>
      <BitOpsToolInner />
    </RightSidebarPanel>
  );
}
