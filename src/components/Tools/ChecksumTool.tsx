import { useCallback, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import RightSidebarPanel from "../RightSidebar/RightSidebarPanel";
import {
  CRC_PRESETS,
  bytesToHex,
  checksum8,
  checksum16,
  checksum32,
  crcPreset,
  crcValueBytes,
  crcWithParams,
  numberToHex,
  stringToBytes,
  xorChecksum,
  type CrcParams,
  type CrcPreset,
  type CrcWidth,
} from "../../utils/checksum";
import { parseByteInput } from "../../utils/byteInput";
import { copyToClipboard } from "../../utils/clipboard";
import styles from "./ChecksumTool.module.css";

type InputMode = "string" | "hex";
type Algorithm =
  | "SUM8"
  | "SUM16"
  | "SUM32"
  | "XOR"
  | "CRC8"
  | "CRC16"
  | "CRC32";
type CrcAction = "calculate" | "verify";

const ALGORITHMS: Algorithm[] = [
  "SUM8",
  "SUM16",
  "SUM32",
  "XOR",
  "CRC8",
  "CRC16",
  "CRC32",
];

function algorithmWidth(algorithm: Algorithm): CrcWidth | null {
  if (algorithm === "CRC8") return 8;
  if (algorithm === "CRC16") return 16;
  if (algorithm === "CRC32") return 32;
  return null;
}

function presetsFor(width: CrcWidth): CrcPreset[] {
  return (Object.keys(CRC_PRESETS) as CrcPreset[])
    .filter((name) => CRC_PRESETS[name].params.width === width);
}

function parseParam(source: string, width: CrcWidth): number | null {
  const text = source.trim();
  if (!/^(?:0[xX][0-9a-fA-F]+|\d+)$/.test(text)) return null;
  const value = Number(text.toLowerCase().startsWith("0x") ? Number.parseInt(text.slice(2), 16) : Number(text));
  const max = width === 32 ? 0xFFFFFFFF : (2 ** width) - 1;
  return Number.isSafeInteger(value) && value >= 0 && value <= max ? value : null;
}

function readTrailing(
  bytes: Uint8Array,
  width: CrcWidth,
  byteOrder: "be" | "le",
): number {
  const count = width / 8;
  let value = 0;
  if (byteOrder === "be") {
    for (let index = bytes.length - count; index < bytes.length; index += 1) {
      value = (value * 256 + bytes[index]) >>> 0;
    }
  } else {
    for (let index = bytes.length - 1; index >= bytes.length - count; index -= 1) {
      value = (value * 256 + bytes[index]) >>> 0;
    }
  }
  return value >>> 0;
}

export function ChecksumToolInner() {
  const { t } = useTranslation();
  const [inputMode, setInputMode] = useState<InputMode>("string");
  const [inputText, setInputText] = useState("");
  const [algorithm, setAlgorithm] = useState<Algorithm>("SUM8");
  const [preset, setPreset] = useState<CrcPreset>("CRC-16/MODBUS");
  const [customCrc, setCustomCrc] = useState(false);
  const [crcAction, setCrcAction] = useState<CrcAction>("calculate");
  const [crcByteOrder, setCrcByteOrder] = useState<"be" | "le">("be");
  const [customPoly, setCustomPoly] = useState("0x8005");
  const [customInit, setCustomInit] = useState("0xFFFF");
  const [customXorOut, setCustomXorOut] = useState("0x0000");
  const [customRefIn, setCustomRefIn] = useState(true);
  const [customRefOut, setCustomRefOut] = useState(true);
  const [copied, setCopied] = useState(false);

  const crcWidth = algorithmWidth(algorithm);
  const availablePresets = useMemo(
    () => crcWidth ? presetsFor(crcWidth) : [],
    [crcWidth],
  );

  const activePreset = useMemo(() => {
    if (!crcWidth) return null;
    if (
      CRC_PRESETS[preset]
      && CRC_PRESETS[preset].params.width === crcWidth
    ) return preset;
    return availablePresets[0] ?? null;
  }, [availablePresets, crcWidth, preset]);

  const inputOutcome = useMemo(() => {
    if (!inputText.trim()) return null;
    if (inputMode === "string") {
      const bytes = stringToBytes(inputText);
      return { ok: true as const, value: { bytes, normalizedHex: bytesToHex(bytes) } };
    }
    return parseByteInput(inputText);
  }, [inputMode, inputText]);

  const customParams = useMemo<CrcParams | null>(() => {
    if (!crcWidth) return null;
    const poly = parseParam(customPoly, crcWidth);
    const init = parseParam(customInit, crcWidth);
    const xorOut = parseParam(customXorOut, crcWidth);
    if (poly === null || init === null || xorOut === null) return null;
    return {
      poly,
      init,
      xorOut,
      refIn: customRefIn,
      refOut: customRefOut,
      width: crcWidth,
    };
  }, [
    crcWidth,
    customInit,
    customPoly,
    customRefIn,
    customRefOut,
    customXorOut,
  ]);

  const result = useMemo(() => {
    if (!inputOutcome?.ok || inputOutcome.value.bytes.length === 0) return null;
    const bytes = inputOutcome.value.bytes;

    if (algorithm === "SUM8") {
      const value = checksum8(bytes);
      return { label: "SUM8", hex: numberToHex(value, 8), dec: String(value) };
    }
    if (algorithm === "SUM16") {
      const value = checksum16(bytes);
      return { label: "SUM16", hex: numberToHex(value, 16), dec: String(value) };
    }
    if (algorithm === "SUM32") {
      const value = checksum32(bytes);
      return { label: "SUM32", hex: numberToHex(value, 32), dec: String(value) };
    }
    if (algorithm === "XOR") {
      const value = xorChecksum(bytes);
      return { label: "XOR", hex: numberToHex(value, 8), dec: String(value) };
    }
    if (!crcWidth || !activePreset) return null;

    const params = customCrc ? customParams : CRC_PRESETS[activePreset].params;
    if (!params) return { error: "invalidCrcParameters" as const };

    const byteCount = crcWidth / 8;
    if (crcAction === "verify" && inputMode === "hex") {
      if (bytes.length <= byteCount) return { error: "crcFrameTooShort" as const };
      const payload = bytes.slice(0, bytes.length - byteCount);
      const received = readTrailing(bytes, crcWidth, crcByteOrder);
      const calculated = crcWithParams(payload, params);
      return {
        label: customCrc ? "Custom CRC-" + crcWidth : activePreset,
        hex: numberToHex(calculated, crcWidth),
        dec: String(calculated >>> 0),
        valid: received === calculated,
        receivedHex: numberToHex(received, crcWidth),
        width: crcWidth,
      };
    }

    const calculated = customCrc
      ? crcWithParams(bytes, params)
      : crcPreset(bytes, activePreset);
    return {
      label: customCrc ? "Custom CRC-" + crcWidth : activePreset,
      hex: numberToHex(calculated, crcWidth),
      dec: String(calculated >>> 0),
      width: crcWidth,
    };
  }, [
    activePreset,
    algorithm,
    crcAction,
    crcByteOrder,
    crcWidth,
    customCrc,
    customParams,
    inputMode,
    inputOutcome,
  ]);

  const handleCopy = useCallback(async () => {
    if (!result || "error" in result || !("hex" in result)) return;
    await copyToClipboard(result.hex);
    setCopied(true);
    window.setTimeout(() => setCopied(false), 1500);
  }, [result]);

  const handleAppend = useCallback(async (order: "be" | "le") => {
    if (
      !result
      || "error" in result
      || !("hex" in result)
      || !result.width
      || !inputOutcome?.ok
    ) return;
    const value = Number.parseInt(result.hex, 16) >>> 0;
    const appended = new Uint8Array(
      inputOutcome.value.bytes.length + result.width / 8,
    );
    appended.set(inputOutcome.value.bytes, 0);
    appended.set(
      crcValueBytes(value, result.width, order),
      inputOutcome.value.bytes.length,
    );
    await copyToClipboard(bytesToHex(appended));
  }, [inputOutcome, result]);

  const selectAlgorithm = useCallback((next: Algorithm) => {
    setAlgorithm(next);
    const width = algorithmWidth(next);
    if (width) {
      const first = presetsFor(width)[0];
      if (first) setPreset(first);
    }
  }, []);

  return (
    <div className={styles.container}>
      <div className={styles.modeRow + " liquid-selector-strip"}>
        <button
          className={styles.modeBtn + " liquid-glass-button liquid-selector-button " + (inputMode === "string" ? "active" : "")}
          onClick={() => setInputMode("string")}
          type="button"
          aria-pressed={inputMode === "string"}
        >
          {t("tools.stringMode")}
        </button>
        <button
          className={styles.modeBtn + " liquid-glass-button liquid-selector-button " + (inputMode === "hex" ? "active" : "")}
          onClick={() => setInputMode("hex")}
          type="button"
          aria-pressed={inputMode === "hex"}
        >
          {t("tools.hexMode")}
        </button>
      </div>

      {inputMode === "string" && (
        <div className={styles.toolHint}>{t("tools.utf8InputHint")}</div>
      )}

      <textarea
        className={styles.input + " liquid-glass-input liquid-glass-textarea"}
        value={inputText}
        onChange={(event) => setInputText(event.target.value)}
        placeholder={
          inputMode === "string"
            ? t("tools.checksumInputPlaceholder")
            : t("tools.checksumHexPlaceholder")
        }
        rows={3}
        spellCheck={false}
      />

      {inputOutcome?.ok && (
        <div className={styles.parsedInfo}>
          <span className={styles.label}>{t("tools.parsedBytes")}:</span>
          <code className={styles.code}>{inputOutcome.value.normalizedHex}</code>
          <span className={styles.len}>{inputOutcome.value.bytes.length} B</span>
        </div>
      )}

      {inputOutcome && !inputOutcome.ok && (
        <div className={styles.parseError}>
          {t("tools.errors." + inputOutcome.error.code, {
            detail: inputOutcome.error.detail ?? "",
            defaultValue: inputOutcome.error.code,
          })}
        </div>
      )}

      <div className={styles.algRow + " liquid-selector-strip"}>
        {ALGORITHMS.map((item) => (
          <button
            key={item}
            className={styles.algBtn + " liquid-glass-button liquid-selector-button " + (algorithm === item ? "active" : "")}
            onClick={() => selectAlgorithm(item)}
            type="button"
            aria-pressed={algorithm === item}
          >
            {item}
          </button>
        ))}
      </div>

      {crcWidth && activePreset && (
        <>
          <div className={styles.presetRow}>
            <label className={styles.label}>{t("tools.preset")}:</label>
            <select
              className={styles.select + " liquid-glass-input liquid-glass-select"}
              value={activePreset}
              disabled={customCrc}
              onChange={(event) => setPreset(event.target.value as CrcPreset)}
            >
              {availablePresets.map((name) => (
                <option key={name} value={name}>{name}</option>
              ))}
            </select>
            <label className={styles.checkboxLabel}>
              <input
                type="checkbox"
                checked={customCrc}
                onChange={(event) => setCustomCrc(event.target.checked)}
              />
              {t("tools.customCrc")}
            </label>
          </div>

          {customCrc && (
            <div className={styles.customGrid}>
              <label>
                Poly
                <input className="liquid-glass-input" value={customPoly} onChange={(event) => setCustomPoly(event.target.value)} />
              </label>
              <label>
                Init
                <input className="liquid-glass-input" value={customInit} onChange={(event) => setCustomInit(event.target.value)} />
              </label>
              <label>
                XorOut
                <input className="liquid-glass-input" value={customXorOut} onChange={(event) => setCustomXorOut(event.target.value)} />
              </label>
              <label className={styles.checkboxLabel}>
                <input type="checkbox" checked={customRefIn} onChange={(event) => setCustomRefIn(event.target.checked)} />
                RefIn
              </label>
              <label className={styles.checkboxLabel}>
                <input type="checkbox" checked={customRefOut} onChange={(event) => setCustomRefOut(event.target.checked)} />
                RefOut
              </label>
            </div>
          )}

          {!customCrc && (
            <div className={styles.crcMeta}>
              <code>
                Poly 0x{numberToHex(CRC_PRESETS[activePreset].params.poly, crcWidth)}
                {" · "}Init 0x{numberToHex(CRC_PRESETS[activePreset].params.init, crcWidth)}
                {" · "}XorOut 0x{numberToHex(CRC_PRESETS[activePreset].params.xorOut, crcWidth)}
                {" · "}Check 0x{numberToHex(CRC_PRESETS[activePreset].check, crcWidth)}
              </code>
            </div>
          )}

          {inputMode === "hex" && (
            <div className={styles.actionRow}>
              <div className={styles.modeRow + " liquid-selector-strip"}>
                <button
                  className={styles.modeBtn + " liquid-glass-button liquid-selector-button " + (crcAction === "calculate" ? "active" : "")}
                  onClick={() => setCrcAction("calculate")}
                  type="button"
                >
                  {t("tools.crcCalculate")}
                </button>
                <button
                  className={styles.modeBtn + " liquid-glass-button liquid-selector-button " + (crcAction === "verify" ? "active" : "")}
                  onClick={() => setCrcAction("verify")}
                  type="button"
                >
                  {t("tools.crcVerifyTrailing")}
                </button>
              </div>
              {crcAction === "verify" && (
                <select
                  className={styles.select + " liquid-glass-input liquid-glass-select"}
                  value={crcByteOrder}
                  onChange={(event) => setCrcByteOrder(event.target.value as "be" | "le")}
                >
                  <option value="be">{t("tools.bigEndian")}</option>
                  <option value="le">{t("tools.littleEndian")}</option>
                </select>
              )}
            </div>
          )}
        </>
      )}

      {result && "error" in result && (
        <div className={styles.parseError}>
          {t("tools.errors." + result.error)}
        </div>
      )}

      {result && !("error" in result) && (
        <div className={styles.resultRow}>
          <div className={styles.resultLabel}>{result.label}:</div>
          <code className={styles.resultHex}>0x{result.hex}</code>
          <span className={styles.resultDec}>({result.dec})</span>
          {"valid" in result && (
            <span className={result.valid ? styles.verifyPass : styles.verifyFail}>
              {result.valid
                ? t("tools.checkPassed")
                : t("tools.crcMismatch", { expected: "0x" + result.hex })}
              {!result.valid && result.receivedHex
                ? " · " + t("tools.receivedValue", { value: "0x" + result.receivedHex })
                : ""}
            </span>
          )}
          <button
            className={styles.copyBtn + " liquid-glass-ghost-button"}
            onClick={handleCopy}
            type="button"
          >
            {copied ? t("tools.copied") : t("common.copy")}
          </button>
        </div>
      )}

      {result && !("error" in result) && result.width && crcAction === "calculate" && inputMode === "hex" && (
        <div className={styles.appendRow}>
          <button className="liquid-glass-ghost-button" type="button" onClick={() => handleAppend("be")}>
            {t("tools.copyAppendedBe")}
          </button>
          <button className="liquid-glass-ghost-button" type="button" onClick={() => handleAppend("le")}>
            {t("tools.copyAppendedLe")}
          </button>
        </div>
      )}

      {!inputText.trim() && (
        <div className={styles.placeholder}>{t("tools.checksumHint")}</div>
      )}
    </div>
  );
}

export default function ChecksumTool() {
  const { t } = useTranslation();
  return (
    <RightSidebarPanel title={t("tools.checksum")}>
      <ChecksumToolInner />
    </RightSidebarPanel>
  );
}
