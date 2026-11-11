/**
 * Terminal screen contract: bytes -> cells.
 *
 * TauTerm deliberately never parses or rewrites PTY output (xterm.js owns every
 * VT semantic), so the only thing the host can get wrong on this path is the
 * terminal *configuration* it hands to xterm. That is not hypothetical: half-block
 * pixel-art logos are positioned with literal leading spaces and coloured through
 * per-cell 24-bit SGR pairs (foreground = upper pixel, background = lower pixel),
 * so one wrong option — `convertEol` rewriting a bare LF, a shifted column origin,
 * a dropped or duplicated row — shreds the artwork while every other assertion in
 * the repository still passes.
 *
 * This replays a captured antigravity CLI startup stream through
 * `@xterm/headless` with the host's exact options and pins the resulting cell
 * grid. The fixture is raw terminal output captured from a real PTY; it is a
 * rendering input, not third-party source.
 */
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import pkg from "@xterm/headless";

const { Terminal } = pkg;

const FIXTURE = resolve(import.meta.dirname, "..", "tests", "fixtures", "agy-half-block-logo.bin");
const UPPER_HALF_BLOCK = "\u2580";
const LOWER_HALF_BLOCK = "\u2584";
/** `Attributes.CM_RGB`: the colour mode bitmask xterm reports for 24-bit SGR. */
const CM_RGB = 0x03000000;

/** Host options that can influence the parsed grid; kept identical to Terminal.tsx. */
const HOST_OPTIONS = {
  convertEol: false,
  allowTransparency: true,
  customGlyphs: true,
  allowProposedApi: true,
  scrollback: 0,
};

/** Expected artwork: leading spaces, glyph run, inner gap, trailing glyph run and metadata. */
const ART_ROWS = [
  { lead: 6, run: LOWER_HALF_BLOCK + UPPER_HALF_BLOCK + UPPER_HALF_BLOCK + LOWER_HALF_BLOCK, gap: 8, text: "Antigravity CLI 1.5.5" },
  { lead: 5, run: UPPER_HALF_BLOCK.repeat(6), gap: 7, text: "myshow1029@gmail.com" },
  { lead: 4, run: UPPER_HALF_BLOCK.repeat(8), gap: 6, text: "C:/work/workspace/novel-forge" },
  { lead: 3, run: LOWER_HALF_BLOCK + UPPER_HALF_BLOCK.repeat(2), gap: 4, tail: UPPER_HALF_BLOCK.repeat(2) + LOWER_HALF_BLOCK },
  { lead: 2, run: LOWER_HALF_BLOCK + UPPER_HALF_BLOCK.repeat(2), gap: 6, tail: UPPER_HALF_BLOCK.repeat(2) + LOWER_HALF_BLOCK },
];

function expectedText(row) {
  return " ".repeat(row.lead) + row.run + " ".repeat(row.gap) + (row.tail ?? row.text);
}

async function replay(bytes, overrides = {}) {
  const terminal = new Terminal({ cols: 120, rows: 30, ...HOST_OPTIONS, ...overrides });
  await new Promise((done) => terminal.write(bytes, done));
  return terminal;
}

function rowText(terminal, y) {
  const line = terminal.buffer.active.getLine(y);
  assert.ok(line, `buffer row ${y} is missing`);
  return line.translateToString(true);
}

function cellAt(terminal, y, x) {
  const line = terminal.buffer.active.getLine(y);
  assert.ok(line, `buffer row ${y} is missing`);
  const cell = line.getCell(x);
  assert.ok(cell, `buffer cell ${y},${x} is missing`);
  return cell;
}

const rgb = (value) => `#${(value & 0xffffff).toString(16).padStart(6, "0")}`;

function assertTruecolor(cell, label, kind, expected) {
  const mode = kind === "fg" ? cell.getFgColorMode() : cell.getBgColorMode();
  const value = kind === "fg" ? cell.getFgColor() : cell.getBgColor();
  assert.equal(mode, CM_RGB, `${label}: ${kind} must be 24-bit truecolor`);
  assert.equal(rgb(value), expected, `${label}: ${kind} colour drifted`);
}

const bytes = new Uint8Array(readFileSync(FIXTURE));
const terminal = await replay(bytes);
const buffer = terminal.buffer.active;

// The artwork and its metadata share a row; locate it, then pin every row of the logo.
let logoRow = -1;
for (let y = 0; y < Math.min(buffer.length, 30); y++) {
  if (rowText(terminal, y).includes("Antigravity CLI 1.5.5")) {
    logoRow = y;
    break;
  }
}
assert.ok(logoRow >= 1, "the captured logo row was not parsed into the buffer");

for (const [index, row] of ART_ROWS.entries()) {
  const actual = rowText(terminal, logoRow + index);
  const expected = expectedText(row);
  assert.equal(actual, expected, `logo row ${index + 1} drifted`);
}

// Leading indentation is what centres the triangle: a dropped space shears the
// whole logo, which is exactly how it failed in the field.
for (const [index, row] of ART_ROWS.entries()) {
  const actual = rowText(terminal, logoRow + index);
  const lead = actual.length - actual.trimStart().length;
  assert.equal(lead, row.lead, `logo row ${index + 1} lost ${row.lead - lead} leading space(s)`);
}

// A half-block cell carries two pixels: foreground is the upper one, background
// the lower one. Both halves must survive as explicit 24-bit colours, because the
// logo's gradient lives in the background half as much as in the foreground half.
const firstLower = cellAt(terminal, logoRow, ART_ROWS[0].lead);
assert.equal(firstLower.getChars(), LOWER_HALF_BLOCK);
assertTruecolor(firstLower, "logo row 1 lower half", "fg", "#dbb131");
assert.ok(firstLower.isBgDefault(), "logo row 1 lower half must not paint a background");

const firstUpper = cellAt(terminal, logoRow, ART_ROWS[0].lead + 1);
assert.equal(firstUpper.getChars(), UPPER_HALF_BLOCK);
assertTruecolor(firstUpper, "logo row 1 upper half", "fg", "#f2922e");
assertTruecolor(firstUpper, "logo row 1 upper half", "bg", "#f6912e");

// `convertEol` is only a translation of LF into CRLF. A PTY stream already carries
// the line endings its own host chose, so the option must stay a no-op for it: the
// captured fixture is CRLF-terminated and has to parse identically either way.
const withConvertEol = await replay(bytes, { convertEol: true });
for (const [index] of ART_ROWS.entries()) {
  assert.equal(
    rowText(withConvertEol, logoRow + index),
    expectedText(ART_ROWS[index]),
    `logo row ${index + 1} must parse identically with convertEol on a CRLF stream`,
  );
}

// PTY line-ending contract: the bundled Windows Console ConPTY engine does not
// forward the client's bytes verbatim. It re-renders the screen and moves between
// rows with a bare LF followed by a relative cursor rewind (CUB), so the row's
// column only survives because a bare LF preserves it. xterm's `convertEol`
// rewrites every LF as CR+LF: the injected CR clamps the cursor to column 0 and
// the following rewind becomes a no-op, collapsing each such row onto column 0 —
// which is exactly how the half-block logo was shredded. Reduced form of the
// captured engine output (capture: bundled engine 1.25, 120x30):
const ENGINE_ROW_A = "\x1b[6C\u2584\u2580\u2580\u2584\x1b[8CAntigravity CLI 1.5.5";
const ENGINE_ROW_B = "\x1b[34D\u2580\u2580\u2580\u2580\u2580\u2580\x1b[7Cmyshow1029@gmail.com";
const engineStream = "\x1b[H" + ENGINE_ROW_A + "\n" + ENGINE_ROW_B;

const ptyLike = await replay(engineStream);
assert.equal(
  rowText(ptyLike, 1),
  "     \u2580\u2580\u2580\u2580\u2580\u2580       myshow1029@gmail.com",
  "a bare LF plus relative rewind must keep the engine row at its intended column",
);

const rewritten = await replay(engineStream, { convertEol: true });
assert.equal(
  rowText(rewritten, 1),
  "\u2580\u2580\u2580\u2580\u2580\u2580       myshow1029@gmail.com",
  "convertEol no longer alters LF+rewind rows: re-evaluate the host option before changing it",
);

// The separator and prompt rows below the artwork must not be displaced either.
const separator = Math.max(...Array.from({ length: 5 }, (_, i) => logoRow + i)) + 2;
assert.match(rowText(terminal, separator), /^\u2500+$/, "the TUI separator row is missing");

console.log(`terminal-screen: pinned ${ART_ROWS.length} logo rows and their 24-bit half-block colours`);
