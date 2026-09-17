import type {
  RemoteDocumentEncoding,
  RemoteDocumentFormat,
  RemoteLineEnding,
} from "../../services/remoteDocumentService";

export const REMOTE_DOCUMENT_ENCODINGS: RemoteDocumentEncoding[] = [
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

export interface DecodedRemoteDocument {
  text: string;
  format: RemoteDocumentFormat;
  binaryLikely: boolean;
}

interface BomDetection {
  encoding: RemoteDocumentEncoding | null;
  bom: boolean;
  offset: number;
}

function detectBom(bytes: Uint8Array): BomDetection {
  if (bytes.length >= 3 && bytes[0] === 0xef && bytes[1] === 0xbb && bytes[2] === 0xbf) {
    return { encoding: "utf-8", bom: true, offset: 3 };
  }
  if (bytes.length >= 2 && bytes[0] === 0xff && bytes[1] === 0xfe) {
    return { encoding: "utf-16le", bom: true, offset: 2 };
  }
  if (bytes.length >= 2 && bytes[0] === 0xfe && bytes[1] === 0xff) {
    return { encoding: "utf-16be", bom: true, offset: 2 };
  }
  return { encoding: null, bom: false, offset: 0 };
}

export function detectRemoteDocumentEncoding(bytes: Uint8Array): {
  encoding: RemoteDocumentEncoding;
  bom: boolean;
} {
  const bom = detectBom(bytes);
  if (bom.encoding) return { encoding: bom.encoding, bom: true };

  try {
    new TextDecoder("utf-8", { fatal: true }).decode(bytes);
    return { encoding: "utf-8", bom: false };
  } catch {
    // 无 BOM 且不是严格 UTF-8 时，不猜测 GB18030/Big5/Shift-JIS 等区域编码。
    // 工程文件误判后保存的风险高于手动选择编码的成本。
    return { encoding: "utf-8", bom: false };
  }
}

export function looksLikeBinary(bytes: Uint8Array): boolean {
  if (bytes.length === 0) return false;
  const bom = detectBom(bytes);
  if (bom.encoding === "utf-16le" || bom.encoding === "utf-16be") return false;

  const sample = bytes.subarray(0, Math.min(bytes.length, 4096));
  let controls = 0;
  for (const byte of sample) {
    if (byte === 0) controls += 3;
    else if (byte < 0x09 || (byte > 0x0d && byte < 0x20)) controls += 1;
  }
  return controls / sample.length > 0.08;
}

export function decodeRemoteDocument(
  bytes: Uint8Array,
  encoding: RemoteDocumentEncoding,
): string {
  const bom = detectBom(bytes);
  const source = bom.encoding === encoding ? bytes.subarray(bom.offset) : bytes;
  try {
    return new TextDecoder(encoding, { fatal: false }).decode(source);
  } catch {
    return new TextDecoder("utf-8", { fatal: false }).decode(source);
  }
}

export function detectLineEnding(text: string): RemoteLineEnding {
  let crlf = 0;
  let lf = 0;
  let cr = 0;
  for (let index = 0; index < text.length; index += 1) {
    if (text[index] === "\r") {
      if (text[index + 1] === "\n") {
        crlf += 1;
        index += 1;
      } else {
        cr += 1;
      }
    } else if (text[index] === "\n") {
      lf += 1;
    }
  }
  if (crlf >= lf && crlf >= cr && crlf > 0) return "crlf";
  if (cr > lf && cr > 0) return "cr";
  return "lf";
}

export function normalizeDocumentText(text: string): string {
  return text.replace(/\r\n/g, "\n").replace(/\r/g, "\n");
}

export function decodeInitialRemoteDocument(bytes: Uint8Array): DecodedRemoteDocument {
  const detected = detectRemoteDocumentEncoding(bytes);
  const decoded = decodeRemoteDocument(bytes, detected.encoding);
  return {
    text: normalizeDocumentText(decoded),
    format: {
      encoding: detected.encoding,
      lineEnding: detectLineEnding(decoded),
      bom: detected.bom,
    },
    binaryLikely: looksLikeBinary(bytes),
  };
}

export function formatRemoteDocumentHex(bytes: Uint8Array): string {
  const lines: string[] = [];
  for (let offset = 0; offset < bytes.length; offset += 16) {
    const chunk = bytes.subarray(offset, offset + 16);
    const hex = Array.from(chunk, (value) => value.toString(16).padStart(2, "0"))
      .join(" ")
      .padEnd(16 * 3 - 1, " ");
    const ascii = Array.from(chunk, (value) =>
      value >= 0x20 && value <= 0x7e ? String.fromCharCode(value) : ".",
    ).join("");
    lines.push(`${offset.toString(16).padStart(8, "0")}  ${hex}  |${ascii}|`);
  }
  return lines.join("\n");
}
