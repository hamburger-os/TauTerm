/**
 * 远程文件预览
 *
 * 数据源始终是后端限制大小的原始字节。文本视图支持常见工程编码手动切换，
 * HEX 视图保留真实字节，不把解码替换字符误当成文件内容。
 */
import { useCallback, useEffect, useMemo, useState } from "react";
import { createPortal } from "react-dom";
import { useTranslation } from "react-i18next";
import Icon from "../common/Icon";
import { formatBytes } from "../../utils/format";
import styles from "./FilePreviewModal.module.css";

type PreviewMode = "text" | "hex";
type PreviewEncoding =
  | "utf-8"
  | "utf-16le"
  | "utf-16be"
  | "gb18030"
  | "big5"
  | "shift_jis"
  | "euc-jp"
  | "euc-kr"
  | "windows-1252";

const ENCODINGS: PreviewEncoding[] = [
  "utf-8",
  "utf-16le",
  "utf-16be",
  "gb18030",
  "big5",
  "shift_jis",
  "euc-jp",
  "euc-kr",
  "windows-1252",
];

const HEX_RENDER_LIMIT = 128 * 1024;

interface FilePreviewModalProps {
  visible: boolean;
  fileName: string;
  data: number[] | null;
  loading: boolean;
  error: string | null;
  fileSize: number;
  onClose: () => void;
}

function detectEncoding(bytes: Uint8Array): PreviewEncoding {
  if (bytes.length >= 3 && bytes[0] === 0xef && bytes[1] === 0xbb && bytes[2] === 0xbf) {
    return "utf-8";
  }
  if (bytes.length >= 2 && bytes[0] === 0xff && bytes[1] === 0xfe) return "utf-16le";
  if (bytes.length >= 2 && bytes[0] === 0xfe && bytes[1] === 0xff) return "utf-16be";

  try {
    new TextDecoder("utf-8", { fatal: true }).decode(bytes);
    return "utf-8";
  } catch {
    // 无 BOM 且不是严格 UTF-8 时不冒充自动识别其他区域编码；保留 UTF-8
    // 作为明确默认值，让用户从工程常用编码列表中选择。
    return "utf-8";
  }
}

function looksBinary(bytes: Uint8Array): boolean {
  if (bytes.length === 0) return false;
  const sample = bytes.subarray(0, Math.min(bytes.length, 4096));
  let controls = 0;
  for (const byte of sample) {
    if (byte === 0) controls += 3;
    else if (byte < 0x09 || (byte > 0x0d && byte < 0x20)) controls += 1;
  }
  return controls / sample.length > 0.08;
}

function decodeBytes(bytes: Uint8Array, encoding: PreviewEncoding): string {
  try {
    return new TextDecoder(encoding, { fatal: false }).decode(bytes);
  } catch {
    return new TextDecoder("utf-8", { fatal: false }).decode(bytes);
  }
}

function formatHex(bytes: Uint8Array): { text: string; truncated: boolean } {
  const visible = bytes.subarray(0, Math.min(bytes.length, HEX_RENDER_LIMIT));
  const lines: string[] = [];
  for (let offset = 0; offset < visible.length; offset += 16) {
    const chunk = visible.subarray(offset, offset + 16);
    const hex = Array.from(chunk, (value) => value.toString(16).padStart(2, "0"))
      .join(" ")
      .padEnd(16 * 3 - 1, " ");
    const ascii = Array.from(chunk, (value) =>
      value >= 0x20 && value <= 0x7e ? String.fromCharCode(value) : ".",
    ).join("");
    lines.push(`${offset.toString(16).padStart(8, "0")}  ${hex}  |${ascii}|`);
  }
  return { text: lines.join("\n"), truncated: bytes.length > visible.length };
}

export default function FilePreviewModal({
  visible,
  fileName,
  data,
  loading,
  error,
  fileSize,
  onClose,
}: FilePreviewModalProps) {
  const { t } = useTranslation();
  const bytes = useMemo(() => new Uint8Array(data ?? []), [data]);
  const [mode, setMode] = useState<PreviewMode>("text");
  const [encoding, setEncoding] = useState<PreviewEncoding>("utf-8");

  useEffect(() => {
    if (!data) return;
    setEncoding(detectEncoding(bytes));
    setMode(looksBinary(bytes) ? "hex" : "text");
  }, [bytes, data]);

  const text = useMemo(() => decodeBytes(bytes, encoding), [bytes, encoding]);
  const hex = useMemo(() => formatHex(bytes), [bytes]);
  const truncated = data !== null && fileSize > bytes.length;
  const lineCount = text ? text.split("\n").length : 0;

  const handleOverlayClick = useCallback(
    (event: React.MouseEvent) => {
      if (event.target === event.currentTarget) onClose();
    },
    [onClose],
  );

  useEffect(() => {
    if (!visible) return;
    const handler = (event: KeyboardEvent) => {
      if (event.key === "Escape") onClose();
    };
    document.addEventListener("keydown", handler);
    return () => document.removeEventListener("keydown", handler);
  }, [visible, onClose]);

  if (!visible) return null;

  return createPortal(
    <div className={`${styles.overlay} glass-overlay`} onClick={handleOverlayClick}>
      <div
        className={`${styles.container} liquid-glass`}
        role="dialog"
        aria-modal="true"
        aria-labelledby="file-preview-title"
      >
        <div className={styles.header}>
          <span id="file-preview-title" className={styles.headerTitle}>{fileName}</span>
          <button
            className={`${styles.closeBtn} liquid-glass-ghost-button`}
            onClick={onClose}
            aria-label={t("common.close")}
          >
            <Icon name="close" size="md" />
          </button>
        </div>

        {data !== null && !loading && !error && (
          <div className={styles.previewToolbar}>
            <div className={styles.modeGroup} role="group" aria-label={t("fileManager.previewMode")}>
              <button
                type="button"
                className={`${styles.modeButton} ${mode === "text" ? styles.modeButtonActive : ""} liquid-glass-ghost-button`}
                aria-pressed={mode === "text"}
                onClick={() => setMode("text")}
              >
                {t("fileManager.previewText")}
              </button>
              <button
                type="button"
                className={`${styles.modeButton} ${mode === "hex" ? styles.modeButtonActive : ""} liquid-glass-ghost-button`}
                aria-pressed={mode === "hex"}
                onClick={() => setMode("hex")}
              >
                HEX
              </button>
            </div>
            {mode === "text" && (
              <label className={styles.encodingControl}>
                <span>{t("fileManager.previewEncoding")}</span>
                <select
                  className={`${styles.encodingSelect} liquid-glass-input liquid-glass-select`}
                  value={encoding}
                  onChange={(event) => setEncoding(event.target.value as PreviewEncoding)}
                >
                  {ENCODINGS.map((value) => (
                    <option key={value} value={value}>{value.toUpperCase()}</option>
                  ))}
                </select>
              </label>
            )}
          </div>
        )}

        <div className={styles.body}>
          {loading && <div className={styles.loading}>{t("fileManager.loading")}</div>}
          {error && <div className={styles.error}>{error}</div>}
          {!loading && !error && data !== null && (
            <>
              {truncated && (
                <div className={styles.notice}>
                  {t("fileManager.previewTruncated", {
                    shown: formatBytes(bytes.length),
                    total: formatBytes(fileSize),
                  })}
                </div>
              )}
              {mode === "text" ? (
                <pre className={styles.previewArea}>{text}</pre>
              ) : (
                <>
                  {hex.truncated && (
                    <div className={styles.notice}>
                      {t("fileManager.previewHexLimited", { size: formatBytes(HEX_RENDER_LIMIT) })}
                    </div>
                  )}
                  <pre className={`${styles.previewArea} ${styles.hexArea}`}>{hex.text}</pre>
                </>
              )}
            </>
          )}
        </div>

        {data !== null && !loading && !error && (
          <div className={styles.statusBar}>
            <span className={styles.statusItem}>{formatBytes(fileSize)}</span>
            <span className={styles.statusItem}>
              {mode === "text"
                ? t("fileManager.lines", { count: lineCount })
                : t("fileManager.previewBytes", { count: bytes.length })}
            </span>
            {mode === "text" && (
              <span className={styles.statusItem}>{encoding.toUpperCase()}</span>
            )}
          </div>
        )}
      </div>
    </div>,
    document.body,
  );
}
