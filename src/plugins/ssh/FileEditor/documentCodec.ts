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
  encodingConfirmed: boolean;
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

export function detectRemoteDocumentEncoding(
  bytes: Uint8Array,
  truncated = false,
): {
  encoding: RemoteDocumentEncoding;
  bom: boolean;
  confirmed: boolean;
} {
  const bom = detectBom(bytes);
  if (bom.encoding) return { encoding: bom.encoding, bom: true, confirmed: true };

  try {
    // A bounded prefix can end in the middle of one UTF-8 code point. Streaming decode keeps
    // that incomplete tail pending while still rejecting malformed bytes inside the snapshot.
    new TextDecoder("utf-8", { fatal: true }).decode(bytes, { stream: truncated });
    return { encoding: "utf-8", bom: false, confirmed: true };
  } catch {
    // 无 BOM 且不是严格 UTF-8 时，不猜测 GB18030/Big5/Shift-JIS 等区域编码。
    // 先保持只读，直到用户明确选择源编码，避免把替换字符保存回工程文件。
    return { encoding: "utf-8", bom: false, confirmed: false };
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
  truncated = false,
): string {
  const bom = detectBom(bytes);
  const source = bom.encoding === encoding ? bytes.subarray(bom.offset) : bytes;
  try {
    // Remote Document never turns malformed source bytes into U+FFFD and later writes those
    // replacements back. A truncated preview may defer only an incomplete final code unit.
    return new TextDecoder(encoding, { fatal: true }).decode(source, { stream: truncated });
  } catch {
    throw new Error(`无法按 ${encoding.toUpperCase()} 严格解码远程文档，请选择正确的源编码`);
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

function decodedDocument(
  bytes: Uint8Array,
  encoding: RemoteDocumentEncoding,
  bom: boolean,
  truncated: boolean,
): DecodedRemoteDocument {
  const decoded = decodeRemoteDocument(bytes, encoding, truncated);
  return {
    text: normalizeDocumentText(decoded),
    format: {
      encoding,
      lineEnding: detectLineEnding(decoded),
      bom,
    },
    binaryLikely: false,
    encodingConfirmed: true,
  };
}

export function decodeRemoteDocumentAs(
  bytes: Uint8Array,
  encoding: RemoteDocumentEncoding,
  truncated = false,
): DecodedRemoteDocument {
  const bom = detectBom(bytes);
  return decodedDocument(bytes, encoding, bom.encoding === encoding && bom.bom, truncated);
}

export function decodeInitialRemoteDocument(
  bytes: Uint8Array,
  truncated = false,
): DecodedRemoteDocument {
  const detected = detectRemoteDocumentEncoding(bytes, truncated);
  const binaryLikely = looksLikeBinary(bytes);
  if (!detected.confirmed) {
    return {
      text: "",
      format: {
        encoding: detected.encoding,
        lineEnding: "lf",
        bom: detected.bom,
      },
      binaryLikely,
      encodingConfirmed: false,
    };
  }

  try {
    return {
      ...decodedDocument(bytes, detected.encoding, detected.bom, truncated),
      binaryLikely,
    };
  } catch {
    // BOM/UTF-8 detection is only a candidate until strict decoding succeeds. Corrupt text stays
    // viewable as HEX and can never become editable through replacement-character decoding.
    return {
      text: "",
      format: {
        encoding: detected.encoding,
        lineEnding: "lf",
        bom: detected.bom,
      },
      binaryLikely,
      encodingConfirmed: false,
    };
  }
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
