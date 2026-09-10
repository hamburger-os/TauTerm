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
  hasLineBreak: false,
  isLargePaste: false,
  contentLineCount: 1,
  characterCount: 10,
});
assert.equal(analyzeTerminalPaste("echo hello\n").requiresConfirmation, true);
assert.equal(analyzeTerminalPaste("echo hello\r\n").hasLineBreak, true);
assert.equal(analyzeTerminalPaste("echo one\necho two").requiresConfirmation, true);
assert.equal(analyzeTerminalPaste("echo one\r\necho two\r\n").requiresConfirmation, true);
assert.equal(analyzeTerminalPaste("echo one\n\n  \necho two").requiresConfirmation, true);
assert.equal(analyzeTerminalPaste("\n\n  \n").requiresConfirmation, true);
assert.equal(analyzeTerminalPaste("x".repeat(5 * 1024)).isLargePaste, false);
assert.equal(analyzeTerminalPaste("x".repeat(5 * 1024 + 1)).isLargePaste, true);

const preview = buildTerminalPastePreview(
  "1\n2\n3\n4\n5\n6\n7\n8\n9",
  8,
  1200,
);
assert.equal(preview.preview, "1\n2\n3\n4\n5\n6\n7\n8");
assert.equal(preview.truncated, true);

const pasteDialogSource = await readFile(
  path.join(ROOT, "src", "components", "Terminal", "PasteSafetyDialog.tsx"),
  "utf8",
);
assert.match(pasteDialogSource, /ConfirmDialog/);
assert.match(pasteDialogSource, /open=\{text !== null\}/);
assert.match(pasteDialogSource, /onConfirm=\{onConfirm\}/);
assert.match(pasteDialogSource, /onCancel=\{onCancel\}/);
assert.match(pasteDialogSource, /terminal\.pasteWarningTitle/);
assert.match(pasteDialogSource, /terminal\.pasteWarningPreview/);

const confirmDialogSource = await readFile(
  path.join(ROOT, "src", "components", "common", "ConfirmDialog.tsx"),
  "utf8",
);
assert.match(confirmDialogSource, /role="alertdialog"/);
assert.match(confirmDialogSource, /GlassButton/);
assert.match(confirmDialogSource, /data-action="cancel"/);
assert.match(confirmDialogSource, /data-action="confirm"/);
assert.match(confirmDialogSource, /variant="ghost"/);
assert.match(confirmDialogSource, /variant=\{intent\}/);
assert.match(confirmDialogSource, /size="md"/);
assert.match(confirmDialogSource, /dialogRef\.current\?\.querySelectorAll/);
assert.match(confirmDialogSource, /t\("common\.cancel"\)/);
assert.match(confirmDialogSource, /t\("common\.confirm"\)/);

const confirmDialogCss = await readFile(
  path.join(ROOT, "src", "components", "common", "ConfirmDialog.module.css"),
  "utf8",
);
assert.match(confirmDialogCss, /border-radius:\s*var\(--radius-xl\)/);
assert.match(confirmDialogCss, /font-size:\s*var\(--text-md\)/);
assert.match(confirmDialogCss, /font-weight:\s*700/);
assert.match(confirmDialogCss, /font-size:\s*var\(--text-sm\)/);

const pasteDialogCss = await readFile(
  path.join(ROOT, "src", "components", "Terminal", "PasteSafetyDialog.module.css"),
  "utf8",
);
assert.match(pasteDialogCss, /font-size:\s*var\(--text-xs\)/);
assert.match(pasteDialogCss, /overflow:\s*auto/);

const terminalSource = await readFile(
  path.join(ROOT, "src", "components", "Terminal", "Terminal.tsx"),
  "utf8",
);
assert.match(terminalSource, /onPasteCapture=\{handlePaste\}/);
assert.match(terminalSource, /term\.paste\(text\)/);
assert.match(terminalSource, /bracketedPasteMode/);
assert.match(terminalSource, /shouldWarnForLineBreak/);
assert.match(terminalSource, /pasteAnalysis\.isLargePaste/);
assert.match(terminalSource, /copyToClipboard\(selection\)\.finally\(restoreTerminalFocus\)/);
assert.match(terminalSource, /requestAnimationFrame\(\(\) => \{[\s\S]*xtermRef\.current\?\.focus\(\)/);
assert.doesNotMatch(terminalSource, /clipboardHasText/);
assert.match(terminalSource, /\(!isConnected \|\| !isActive\) && pendingPaste !== null/);
assert.match(terminalSource, /剪贴板访问必须由明确的 Paste 动作触发/);
assert.match(terminalSource, /Ctrl\+Insert/);
assert.match(terminalSource, /Shift\+Insert/);
assert.match(terminalSource, /Meta\+C/);
assert.match(terminalSource, /Meta\+V/);
assert.doesNotMatch(
  terminalSource,
  /case "paste":[\s\S]{0,220}onData\(/,
  "context-menu paste must not bypass xterm paste semantics",
);

assert.match(terminalSource, /id: "inspectProtocol"/);
assert.match(
  terminalSource,
  /case "inspectProtocol":[\s\S]{0,520}new CustomEvent\("tauterm:protocol-inspect"[\s\S]{0,220}detail: \{ sessionId, input: selection \}/,
  "terminal selection handoff must be explicit and session-scoped",
);

const dualPaneSource = await readFile(
  path.join(ROOT, "src", "components", "Terminal", "DualPane.tsx"),
  "utf8",
);
assert.match(
  dualPaneSource,
  /onContextMenu=\{\(event\) => handleRowContextMenu\(event, line\)\}/,
);
assert.match(
  dualPaneSource,
  /tauterm:protocol-inspect[\s\S]{0,220}sessionId[\s\S]{0,120}contextLine\.hex/,
  "Dual/HEX handoff must use the complete row frame and session identity",
);

const protocolToolSource = await readFile(
  path.join(ROOT, "src", "components", "Tools", "ProtocolTool.tsx"),
  "utf8",
);
assert.match(
  protocolToolSource,
  /detail\?\.sessionId !== sessionId/,
  "Protocol Inspector must ignore handoff events from other sessions",
);

const appSource = await readFile(
  path.join(ROOT, "src", "App.tsx"),
  "utf8",
);
assert.match(
  appSource,
  /setRightSidebarVisible\(true\)[\s\S]{0,300}tauterm:protocol-inspect/,
  "explicit protocol handoff must reveal the engineering sidebar",
);

const networkViewSource = await readFile(
  path.join(ROOT, "src", "components", "Network", "NetworkDebugSessionView.tsx"),
  "utf8",
);
assert.match(networkViewSource, /<DualPane sessionId=\{sessionId\}/);
assert.match(networkViewSource, /function TcpFrameList/);
assert.match(
  networkViewSource,
  /onContextMenu=\{\(event\) => openContextMenu\(event, line\.hex\)\}/,
);
assert.match(
  networkViewSource,
  /tauterm:protocol-inspect[\s\S]{0,180}detail: \{ sessionId, input: contextHex \}/,
);

const udpGridSource = await readFile(
  path.join(ROOT, "src", "components", "Network", "UdpPacketGrid.tsx"),
  "utf8",
);
assert.match(
  udpGridSource,
  /onContextMenu=\{\(event\) => openContextMenu\(event, row\.hex\)\}/,
);
assert.match(
  udpGridSource,
  /tauterm:protocol-inspect[\s\S]{0,180}detail: \{ sessionId, input: contextHex \}/,
);

const registrySource = await readFile(
  path.join(ROOT, "src", "shortcuts", "registry.ts"),
  "utf8",
);
assert.match(registrySource, /TERMINAL_COPY[\s\S]*Ctrl\+Shift\+C/);
assert.match(registrySource, /TERMINAL_PASTE[\s\S]*Ctrl\+Shift\+V/);
assert.match(registrySource, /TERMINAL_RESERVED_KEYS[\s\S]*"Ctrl\+C"[\s\S]*"Ctrl\+V"[\s\S]*"Ctrl\+Insert"[\s\S]*"Shift\+Insert"/);
assert.match(registrySource, /if \(isTerminalReservedShortcut\(pressed\)\) return null/);
assert.match(registrySource, /validIds\.has\(s\.id\) && !isTerminalReservedShortcut\(s\.keys\)/);
assert.doesNotMatch(registrySource, /TERMINAL_COPY, keys: "Ctrl\+C"/);
assert.doesNotMatch(registrySource, /TERMINAL_PASTE, keys: "Ctrl\+V"/);

const shortcutSettingsSource = await readFile(
  path.join(ROOT, "src", "components", "Settings", "panels", "ShortcutSettings.tsx"),
  "utf8",
);
assert.match(shortcutSettingsSource, /isTerminalReservedShortcut\(newKeys\)/);

console.log("terminal-clipboard: shortcuts, paste routing, shared confirmation, focus, safety, and explicit protocol-inspection handoff verified");