/**
 * Canonical hex dump formatting.
 *
 * Three surfaces used to format hex independently (terminal streaming view, RTT HEX
 * view, remote document editor), which made offsets, column widths and ASCII framing
 * drift apart. They all render the same primitive: a fixed-width offset, the byte
 * columns and an ASCII gutter. Layout differences stay explicit parameters.
 *
 * Pure module (no DOM, no React) so it is unit-testable and shared by every view.
 */

export interface HexDumpLayout {
  /** 每行字节数，默认 16 */
  bytesPerRow?: number;
  /**
   * 把字节列拆成两半（终端流式 hex 视图用它保持固定 78 字符行宽，
   * 以便用 `\r` 原地覆盖未完成的行）。默认不拆。
   */
  splitHalf?: boolean;
  /** ASCII 区两侧是否加 `|` 边框，默认 true */
  asciiFrame?: boolean;
  /**
   * 把 ASCII 区补齐到整行宽度。流式终端视图用 `\r` 原地覆盖未完成行时必须开启，
   * 否则较短的行会留下上一行尾部的残字。
   */
  padAscii?: boolean;
}

export const HEX_BYTES_PER_ROW = 16;
const DEFAULT_BYTES_PER_ROW = HEX_BYTES_PER_ROW;
const HALF_COLUMNS = 8;

function toHexByte(value: number): string {
  return value.toString(16).padStart(2, "0");
}

function toPrintableAscii(value: number): string {
  return value >= 0x20 && value <= 0x7e ? String.fromCharCode(value) : ".";
}

function formatOffset(offset: number): string {
  return offset.toString(16).padStart(8, "0");
}

function formatColumns(bytes: Uint8Array, splitHalf: boolean): string[] {
  if (!splitHalf) {
    return [Array.from(bytes, toHexByte).join(" ")];
  }
  const left = Array.from(bytes.subarray(0, HALF_COLUMNS), toHexByte).join(" ");
  const right = Array.from(bytes.subarray(HALF_COLUMNS), toHexByte).join(" ");
  return [left, right];
}

/** Formats one row; `bytes` may be shorter than a full row (last partial row). */
export function formatHexLine(
  bytes: Uint8Array,
  offset: number,
  layout: HexDumpLayout = {},
): string {
  const bytesPerRow = layout.bytesPerRow ?? DEFAULT_BYTES_PER_ROW;
  const row = bytes.subarray(0, bytesPerRow);
  const rawAscii = Array.from(row, toPrintableAscii).join("");
  const ascii = layout.padAscii ? rawAscii.padEnd(bytesPerRow, " ") : rawAscii;
  const framed = layout.asciiFrame === false ? ascii : `|${ascii}|`;

  if (layout.splitHalf) {
    const [left, right] = formatColumns(row, true);
    // Fixed 78-character row (8 offset + 2 + 23 + 2 + 23 + 2 + 16 + 2) so a
    // carriage-return redraw overwrites the previous row without leftovers.
    const halfWidth = HALF_COLUMNS * 3 - 1;
    return `${formatOffset(offset)}  ${left.padEnd(halfWidth, " ")}  ${right.padEnd(halfWidth, " ")}  ${framed}`;
  }

  const width = bytesPerRow * 3 - 1;
  return `${formatOffset(offset)}  ${Array.from(row, toHexByte).join(" ").padEnd(width, " ")}  ${framed}`;
}

/** Formats a whole buffer as one row per `bytesPerRow` bytes. */
export function formatHexLines(
  bytes: Uint8Array,
  offset = 0,
  layout: HexDumpLayout = {},
): string[] {
  const bytesPerRow = layout.bytesPerRow ?? DEFAULT_BYTES_PER_ROW;
  const lines: string[] = [];
  for (let cursor = 0; cursor < bytes.length; cursor += bytesPerRow) {
    lines.push(formatHexLine(bytes.subarray(cursor, cursor + bytesPerRow), offset + cursor, layout));
  }
  return lines;
}

export function formatHexDump(
  bytes: Uint8Array,
  offset = 0,
  layout: HexDumpLayout = {},
): string {
  return formatHexLines(bytes, offset, layout).join("\n");
}
