import fs from "node:fs";
import path from "node:path";

const ROOT = process.cwd();
const SRC = path.join(ROOT, "src");
const CONFIRM_DIALOG = path.join(SRC, "components", "common", "ConfirmDialog.tsx");
const CONFIRM_DIALOG_STYLES = path.join(SRC, "components", "common", "ConfirmDialog.module.css");
const CONNECT_DIALOG_STYLES = path.join(SRC, "components", "Layout", "ConnectDialog.module.css");
const TOKENS = path.join(SRC, "styles", "tokens.css");
const SOURCE_EXTENSIONS = new Set([".ts", ".tsx", ".js", ".jsx"]);
const STYLE_EXTENSIONS = new Set([".css"]);
const CONFIRM_LAYER_OFFSET = 10;

function walk(dir, extensions, files = []) {
  for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
    const full = path.join(dir, entry.name);
    if (entry.isDirectory()) walk(full, extensions, files);
    else if (extensions.has(path.extname(entry.name))) files.push(full);
  }
  return files;
}

function relative(file) {
  return path.relative(ROOT, file).split(path.sep).join("/");
}

function numericToken(source, token) {
  const match = source.match(new RegExp(`--${token}:\\s*(-?\\d+(?:\\.\\d+)?)\\s*;`));
  return match ? Number(match[1]) : null;
}

const sourceFiles = walk(SRC, SOURCE_EXTENSIONS);
const styleFiles = walk(SRC, STYLE_EXTENSIONS);
const errors = [];
const nativeDialogPatterns = [
  { label: "window.confirm", re: /\bwindow\s*\.\s*confirm\s*\(/ },
  { label: "window.alert", re: /\bwindow\s*\.\s*alert\s*\(/ },
  { label: "window.prompt", re: /\bwindow\s*\.\s*prompt\s*\(/ },
  { label: "global confirm", re: /(^|[^.$\w])confirm\s*\(/ },
  { label: "global alert", re: /(^|[^.$\w])alert\s*\(/ },
  { label: "global prompt", re: /(^|[^.$\w])prompt\s*\(/ },
];

for (const file of sourceFiles) {
  const lines = fs.readFileSync(file, "utf8").split(/\r?\n/);
  let inBlockComment = false;
  lines.forEach((line, index) => {
    let code = line;
    if (inBlockComment) {
      const end = code.indexOf("*/");
      if (end < 0) return;
      code = code.slice(end + 2);
      inBlockComment = false;
    }
    for (;;) {
      const blockStart = code.indexOf("/*");
      const lineStart = code.indexOf("//");
      if (lineStart >= 0 && (blockStart < 0 || lineStart < blockStart)) {
        code = code.slice(0, lineStart);
        break;
      }
      if (blockStart < 0) break;
      const blockEnd = code.indexOf("*/", blockStart + 2);
      if (blockEnd < 0) {
        code = code.slice(0, blockStart);
        inBlockComment = true;
        break;
      }
      code = code.slice(0, blockStart) + code.slice(blockEnd + 2);
    }

    for (const pattern of nativeDialogPatterns) {
      if (pattern.re.test(code)) {
        errors.push(`${relative(file)}:${index + 1}: native ${pattern.label} is forbidden; use themed UI`);
      }
    }
  });
}

if (!fs.existsSync(CONFIRM_DIALOG)) {
  errors.push("src/components/common/ConfirmDialog.tsx is missing");
} else {
  const source = fs.readFileSync(CONFIRM_DIALOG, "utf8");
  if (!source.includes('t("common.cancel")')) {
    errors.push("ConfirmDialog must render common.cancel as the fixed cancel action");
  }
  if (!source.includes('t("common.confirm")')) {
    errors.push("ConfirmDialog must render common.confirm as the fixed confirm action");
  }
  if (/\b(confirmLabel|cancelLabel|confirmText|cancelText)\b/.test(source)) {
    errors.push("ConfirmDialog must not expose per-call action-label overrides");
  }
}

if (!fs.existsSync(CONFIRM_DIALOG_STYLES)) {
  errors.push("src/components/common/ConfirmDialog.module.css is missing");
} else {
  const source = fs.readFileSync(CONFIRM_DIALOG_STYLES, "utf8");
  const expectedLayer = new RegExp(
    `z-index\\s*:\\s*calc\\(\\s*var\\(--z-overlay\\)\\s*\\+\\s*${CONFIRM_LAYER_OFFSET}\\s*\\)\\s*;`,
  );
  if (!expectedLayer.test(source)) {
    errors.push(
      `ConfirmDialog must stay one blocking layer above normal overlays with calc(var(--z-overlay) + ${CONFIRM_LAYER_OFFSET})`,
    );
  }
}

if (!fs.existsSync(CONNECT_DIALOG_STYLES)) {
  errors.push("src/components/Layout/ConnectDialog.module.css is missing");
} else {
  const source = fs.readFileSync(CONNECT_DIALOG_STYLES, "utf8");
  if (!/z-index\s*:\s*var\(--z-overlay\)\s*;/.test(source)) {
    errors.push("ConnectDialog must use --z-overlay so nested ConfirmDialog stays interactive above it");
  }
}

if (!fs.existsSync(TOKENS)) {
  errors.push("src/styles/tokens.css is missing");
} else {
  const source = fs.readFileSync(TOKENS, "utf8");
  const overlay = numericToken(source, "z-overlay");
  const toast = numericToken(source, "z-toast");
  if (overlay === null || toast === null) {
    errors.push("tokens.css must define numeric --z-overlay and --z-toast stacking tokens");
  } else {
    const confirm = overlay + CONFIRM_LAYER_OFFSET;
    if (!(overlay < confirm && confirm < toast)) {
      errors.push(
        `stacking contract must remain overlay < nested confirm < toast; got ${overlay} < ${confirm} < ${toast}`,
      );
    }
  }
}

for (const file of styleFiles) {
  if (!file.endsWith("Dialog.module.css")) continue;
  const source = fs.readFileSync(file, "utf8");
  if (/z-index\s*:\s*var\(--z-toast\)\s*;/.test(source)) {
    errors.push(`${relative(file)}: normal dialogs must not occupy --z-toast; reserve it for toast notifications`);
  }
}

const forbiddenPerDialogActionKeys = [
  "fileManager.deleteConfirmAction",
  "terminal.pasteWarningCancel",
  "terminal.pasteWarningConfirm",
];
for (const file of sourceFiles) {
  const source = fs.readFileSync(file, "utf8");
  for (const key of forbiddenPerDialogActionKeys) {
    if (source.includes(key)) {
      errors.push(`${relative(file)}: binary confirmation action must use common.cancel/common.confirm, not ${key}`);
    }
  }
}

if (errors.length > 0) {
  console.error("Confirmation dialog contract check failed:\n");
  for (const error of errors) console.error(`- ${error}`);
  process.exit(1);
}

console.log("Confirmation dialog contract check passed.");
