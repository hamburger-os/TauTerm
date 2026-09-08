import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";
import {
  analyzeTerminalPaste,
  buildTerminalPastePreview,
  normalizeTerminalPasteText,
} from "../src/utils/terminalClipboard.ts";

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");

assert.equal(normalizeTerminalPasteText("a\r\nb\rc"), "a\nb\nc");

assert.deepEqual(analyzeTerminalPaste("echo hello"), {
  requiresConfirmation: false,
  contentLineCount: 1,
  characterCount: 10,
});
assert.equal(analyzeTerminalPaste("echo hello\n").requiresConfirmation, false);
assert.equal(analyzeTerminalPaste("echo hello\r\n").requiresConfirmation, false);
assert.equal(analyzeTerminalPaste("echo one\necho two").requiresConfirmation, true);
assert.equal(analyzeTerminalPaste("echo one\r\necho two\r\n").requiresConfirmation, true);
assert.equal(analyzeTerminalPaste("echo one\n\n  \necho two").requiresConfirmation, true);
assert.equal(analyzeTerminalPaste("\n\n  \n").requiresConfirmation, false);

const preview = buildTerminalPastePreview(
  "1\n2\n3\n4\n5\n6\n7\n8\n9",
  8,
  1200,
);
assert.equal(preview.preview, "1\n2\n3\n4\n5\n6\n7\n8");
assert.equal(preview.truncated, true);

const terminalSource = await readFile(
  path.join(ROOT, "src", "components", "Terminal", "Terminal.tsx"),
  "utf8",
);
assert.match(terminalSource, /onPasteCapture=\{handlePaste\}/);
assert.match(terminalSource, /term\.paste\(text\)/);
assert.match(terminalSource, /Ctrl\+Insert/);
assert.match(terminalSource, /Shift\+Insert/);
assert.match(terminalSource, /Meta\+C/);
assert.match(terminalSource, /Meta\+V/);
assert.doesNotMatch(
  terminalSource,
  /case "paste":[\s\S]{0,220}onData\(/,
  "context-menu paste must not bypass xterm paste semantics",
);

const registrySource = await readFile(
  path.join(ROOT, "src", "shortcuts", "registry.ts"),
  "utf8",
);
assert.match(registrySource, /TERMINAL_COPY[\s\S]*Ctrl\+Shift\+C/);
assert.match(registrySource, /TERMINAL_PASTE[\s\S]*Ctrl\+Shift\+V/);

console.log("terminal-clipboard: shortcuts, paste routing, focus contract and safety analysis verified");
