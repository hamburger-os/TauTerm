import { useState, useMemo } from "react";
import { useTranslation } from "react-i18next";
import RightSidebarPanel from "../RightSidebar/RightSidebarPanel";
import {
  bitwiseOp,
  parseStructDefinition,
  OP_KEYS,
  type BitOp,
} from "../../utils/bitops";
import styles from "./BitOpsTool.module.css";

type ToolMode = "bitwise" | "sizeof";

export function BitOpsToolInner() {
  const { t } = useTranslation();

  const [mode, setMode] = useState<ToolMode>("bitwise");

  // ── 位运算状态 ──
  const [opA, setOpA] = useState("");
  const [opB, setOpB] = useState("");
  const [bitOp, setBitOp] = useState<BitOp>("AND");

  // ── C sizeof 状态 ──
  const [structCode, setStructCode] = useState("");

  // ── 位运算结果 ──
  const bitResult = useMemo(() => {
    const a = parseInt(opA, 10);
    const b = parseInt(opB, 10);
    if (isNaN(a)) return null;
    if (bitOp !== "NOT" && isNaN(b)) return null;
    return bitwiseOp(a, isNaN(b) ? 0 : b, bitOp);
  }, [opA, opB, bitOp]);

  // 无效输入检测
  const bitwiseInputError = useMemo(() => {
    if (!opA.trim() && !opB.trim()) return null;
    if (opA.trim() && isNaN(parseInt(opA, 10))) return "tools.invalidNumber";
    if (bitOp !== "NOT" && opB.trim() && isNaN(parseInt(opB, 10))) return "tools.invalidNumber";
    return null;
  }, [opA, opB, bitOp]);

  // ── C sizeof 结果 ──
  const structResult = useMemo(() => {
    if (!structCode.trim()) return null;
    return parseStructDefinition(structCode);
  }, [structCode]);

  return (
    <div className={styles.container}>
      {/* 模式切换 */}
      <div className={styles.modeRow}>
        <button
          className={`${styles.modeBtn} ${mode === "bitwise" ? styles.active : ""}`}
          onClick={() => setMode("bitwise")}
        >
          {t("tools.bitwiseMode") ?? "Bitwise"}
        </button>
        <button
          className={`${styles.modeBtn} ${mode === "sizeof" ? styles.active : ""}`}
          onClick={() => setMode("sizeof")}
        >
          {t("tools.sizeofMode") ?? "C sizeof"}
        </button>
      </div>

      {/* ── 位运算模式 ── */}
      {mode === "bitwise" && (
        <div className={styles.bitwiseSection}>
          {/* 操作数 A */}
          <div className={styles.opRow}>
            <label className={styles.label}>A:</label>
            <input
              className={`${styles.opInput} liquid-glass-input`}
              value={opA}
              onChange={(e) => setOpA(e.target.value)}
              placeholder={t("tools.bitwiseOperandPlaceholder") ?? "e.g. 170 (0xAA)"}
              spellCheck={false}
            />
          </div>

          {/* 运算符 */}
          <div className={styles.opRow}>
            <label className={styles.label}>{t("tools.operator") ?? "Operator"}:</label>
            <select
              className={`${styles.select} liquid-glass-input liquid-glass-select`}
              value={bitOp}
              onChange={(e) => setBitOp(e.target.value as BitOp)}
            >
              {(OP_KEYS as BitOp[]).map((k) => (
                <option key={k} value={k}>{t(`tools.bitOps.${k}`)}</option>
              ))}
            </select>
          </div>

          {/* 操作数 B (NOT 不需要) */}
          {bitOp !== "NOT" && (
            <div className={styles.opRow}>
              <label className={styles.label}>B:</label>
              <input
                className={`${styles.opInput} liquid-glass-input`}
                value={opB}
                onChange={(e) => setOpB(e.target.value)}
                placeholder={t("tools.bitwiseOperandPlaceholder") ?? "e.g. 15 (0x0F)"}
                spellCheck={false}
              />
            </div>
          )}

          {/* 无效输入错误提示 */}
          {bitwiseInputError && (
            <div className={styles.parseError}>
              {t(bitwiseInputError) ?? "Please enter a valid number"}
            </div>
          )}

          {/* 位可视化结果 */}
          {bitResult && (
            <div className={styles.bitResult}>
              <div className={styles.resultHeader}>
                <span>{t("tools.result") ?? "Result"}:</span>
                <code className={styles.resultVal}>
                  0x{bitResult.hex} ({bitResult.result})
                </code>
              </div>
              <div className={styles.bitsDisplay}>
                {bitResult.bits.split(" ").map((nibble, i) => (
                  <span key={i} className={styles.nibble}>
                    {nibble}
                  </span>
                ))}
              </div>
            </div>
          )}
        </div>
      )}

      {/* ── C sizeof 模式 ── */}
      {mode === "sizeof" && (
        <div className={styles.sizeofSection}>
          <textarea
            className={`${styles.structInput} liquid-glass-input liquid-glass-textarea`}
            value={structCode}
            onChange={(e) => setStructCode(e.target.value)}
            placeholder={
              "struct {\n  char a;\n  int b;\n  char c;\n}"
            }
            rows={6}
            spellCheck={false}
          />

          {/* 解析结果表格 */}
          {structResult && (
            <div className={styles.structResult}>
              <table className={styles.table}>
                <thead>
                  <tr>
                    <th>{t("tools.structMember") ?? "Member"}</th>
                    <th>{t("tools.structType") ?? "Type"}</th>
                    <th>{t("tools.structOffset") ?? "Offset"}</th>
                    <th>{t("tools.structSize") ?? "Size"}</th>
                    <th>{t("tools.structPad") ?? "Padding"}</th>
                  </tr>
                </thead>
                <tbody>
                  {structResult.members.map((m, i) => (
                    <tr key={i}>
                      <td><code>{m.name}</code></td>
                      <td><code>{m.type}</code></td>
                      <td>{m.offset}</td>
                      <td>{m.size}</td>
                      <td className={m.padding > 0 ? styles.padHighlight : ""}>
                        {m.padding > 0 ? m.padding : "-"}
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
              <div className={styles.totalRow}>
                {t("tools.structTotal") ?? "sizeof"} = {structResult.totalSize}
                <span className={styles.alignNote}>
                  (align: {structResult.alignment})
                </span>
              </div>
            </div>
          )}

          {!structResult && structCode.trim() && (
            <div className={styles.parseError}>
              {t("tools.structParseError") ?? "Cannot parse struct definition"}
            </div>
          )}
        </div>
      )}
    </div>
  );
}

export default function BitOpsTool() {
  const { t } = useTranslation();
  return (
    <RightSidebarPanel title={t("tools.bitops") ?? "Bit Operations"}>
      <BitOpsToolInner />
    </RightSidebarPanel>
  );
}
