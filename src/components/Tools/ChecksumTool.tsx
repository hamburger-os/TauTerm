import { useState, useCallback, useMemo } from "react";
import { useTranslation } from "react-i18next";
import RightSidebarPanel from "../RightSidebar/RightSidebarPanel";
import {
  checksum8,
  checksum16,
  xorChecksum,
  crc8,
  crc16,
  crc32,
  stringToBytes,
  parseHexString,
  bytesToHex,
  numberToHex,
  CRC8_PRESETS,
  CRC16_PRESETS,
  CRC32_PRESETS,
  type Crc8Preset,
  type Crc16Preset,
  type Crc32Preset,
} from "../../utils/checksum";
import styles from "./ChecksumTool.module.css";

type InputMode = "string" | "hex";
type Algorithm = "SUM8" | "SUM16" | "XOR" | "CRC8" | "CRC16" | "CRC32";

export function ChecksumToolInner() {
  const { t } = useTranslation();

  const [inputMode, setInputMode] = useState<InputMode>("string");
  const [inputText, setInputText] = useState("");
  const [algorithm, setAlgorithm] = useState<Algorithm>("SUM8");
  const [crc8Preset, setCrc8Preset] = useState<Crc8Preset>("CRC8-Basic");
  const [crc16Preset, setCrc16Preset] = useState<Crc16Preset>("CRC16-Modbus");
  const [crc32Preset, setCrc32Preset] = useState<Crc32Preset>("CRC32");
  const [copied, setCopied] = useState(false);

  // 解析输入为字节数组
  const bytes = useMemo(() => {
    if (!inputText.trim()) return null;
    return inputMode === "string"
      ? stringToBytes(inputText)
      : parseHexString(inputText);
  }, [inputText, inputMode]);

  const parsedHex = useMemo(() => {
    if (!bytes || bytes.length === 0) return null;
    return bytesToHex(bytes);
  }, [bytes]);

  // 计算结果
  const result = useMemo(() => {
    if (!bytes || bytes.length === 0) return null;
    switch (algorithm) {
      case "SUM8": return { label: "SUM8", hex: numberToHex(checksum8(bytes), 8), dec: String(checksum8(bytes)) };
      case "SUM16": return { label: "SUM16", hex: numberToHex(checksum16(bytes), 16), dec: String(checksum16(bytes)) };
      case "XOR": return { label: "XOR", hex: numberToHex(xorChecksum(bytes), 8), dec: String(xorChecksum(bytes)) };
      case "CRC8": {
        const v = crc8(bytes, crc8Preset);
        return { label: `CRC8 (${crc8Preset})`, hex: numberToHex(v, 8), dec: String(v) };
      }
      case "CRC16": {
        const v = crc16(bytes, crc16Preset);
        return { label: `CRC16 (${crc16Preset})`, hex: numberToHex(v, 16), dec: String(v) };
      }
      case "CRC32": {
        const v = crc32(bytes, crc32Preset);
        return { label: `CRC32 (${crc32Preset})`, hex: numberToHex(v, 32), dec: String(v) };
      }
      default: return null;
    }
  }, [bytes, algorithm, crc8Preset, crc16Preset, crc32Preset]);

  const handleCopy = useCallback(async () => {
    if (!result) return;
    try {
      await navigator.clipboard.writeText(result.hex);
      setCopied(true);
      setTimeout(() => setCopied(false), 1500);
    } catch { /* ignore */ }
  }, [result]);

  return (
    <div className={styles.container}>
      {/* 输入模式 */}
      <div className={styles.modeRow}>
        <button
          className={`${styles.modeBtn} ${inputMode === "string" ? styles.active : ""}`}
          onClick={() => setInputMode("string")}
        >
          {t("tools.stringMode") ?? "Text"}
        </button>
        <button
          className={`${styles.modeBtn} ${inputMode === "hex" ? styles.active : ""}`}
          onClick={() => setInputMode("hex")}
        >
          {t("tools.hexMode") ?? "HEX"}
        </button>
      </div>

      {/* 输入文本框 */}
      <textarea
        className={`${styles.input} liquid-glass-input liquid-glass-textarea`}
        value={inputText}
        onChange={(e) => setInputText(e.target.value)}
        placeholder={
          inputMode === "string"
            ? t("tools.checksumInputPlaceholder") ?? "Enter text..."
            : t("tools.checksumHexPlaceholder") ?? "AA BB CC DD or 0xAA,0xBB..."
        }
        rows={3}
        spellCheck={false}
      />

      {/* 解析结果：显示解析后的 HEX */}
      {parsedHex && (
        <div className={styles.parsedInfo}>
          <span className={styles.label}>{t("tools.parsedBytes") ?? "Bytes"}:</span>
          <code className={styles.code}>{parsedHex}</code>
          <span className={styles.len}>{bytes?.length ?? 0} B</span>
        </div>
      )}

      {/* 算法选择 */}
      <div className={styles.algRow}>
        {(["SUM8", "SUM16", "XOR", "CRC8", "CRC16", "CRC32"] as Algorithm[]).map((alg) => (
          <button
            key={alg}
            className={`${styles.algBtn} ${algorithm === alg ? styles.active : ""}`}
            onClick={() => setAlgorithm(alg)}
          >
            {alg === "SUM8" ? "SUM8" : alg === "SUM16" ? "SUM16" : alg}
          </button>
        ))}
      </div>

      {/* CRC 预设选择 */}
      {algorithm === "CRC8" && (
        <div className={styles.presetRow}>
          <label className={styles.label}>{t("tools.preset") ?? "Preset"}:</label>
          <select
            className={`${styles.select} liquid-glass-input liquid-glass-select`}
            value={crc8Preset}
            onChange={(e) => setCrc8Preset(e.target.value as Crc8Preset)}
          >
            {Object.keys(CRC8_PRESETS).map((k) => (
              <option key={k} value={k}>{k}</option>
            ))}
          </select>
        </div>
      )}
      {algorithm === "CRC16" && (
        <div className={styles.presetRow}>
          <label className={styles.label}>{t("tools.preset") ?? "Preset"}:</label>
          <select
            className={`${styles.select} liquid-glass-input liquid-glass-select`}
            value={crc16Preset}
            onChange={(e) => setCrc16Preset(e.target.value as Crc16Preset)}
          >
            {Object.keys(CRC16_PRESETS).map((k) => (
              <option key={k} value={k}>{k}</option>
            ))}
          </select>
        </div>
      )}
      {algorithm === "CRC32" && (
        <div className={styles.presetRow}>
          <label className={styles.label}>{t("tools.preset") ?? "Preset"}:</label>
          <select
            className={`${styles.select} liquid-glass-input liquid-glass-select`}
            value={crc32Preset}
            onChange={(e) => setCrc32Preset(e.target.value as Crc32Preset)}
          >
            {Object.keys(CRC32_PRESETS).map((k) => (
              <option key={k} value={k}>{k}</option>
            ))}
          </select>
        </div>
      )}

      {/* 计算结果 */}
      {result && (
        <div className={styles.resultRow}>
          <div className={styles.resultLabel}>{result.label}:</div>
          <code className={styles.resultHex}>0x{result.hex}</code>
          <span className={styles.resultDec}>({result.dec})</span>
          <button className={styles.copyBtn} onClick={handleCopy} title={t("common.copy") ?? "Copy"}>
            {copied ? t("tools.copied") : t("common.copy")}
          </button>
        </div>
      )}

      {/* 无输入提示 */}
      {!bytes && (
        <div className={styles.placeholder}>
          {t("tools.checksumHint") ?? "Enter text or HEX to calculate"}
        </div>
      )}
    </div>
  );
}

export default function ChecksumTool() {
  const { t } = useTranslation();
  return (
    <RightSidebarPanel title={t("tools.checksum") ?? "Checksum Calc"}>
      <ChecksumToolInner />
    </RightSidebarPanel>
  );
}
