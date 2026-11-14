/**
 * 文件管理器符号链接契约检查。
 *
 * 三件事只允许有一份真源，本脚本把它们钉住：
 * 1. 目录项类型只由后端 `entry_type` 表达，前端不得再出现 `is_dir`；
 * 2. 链接解析/激活映射只在 `entryActivation.ts` 一处调用 `sftp_resolve_entry_cmd`；
 *    列表浏览是单次往返，不在每个链接上做 N+1 stat；
 * 3. 远程文档通过 `resolvedPath` / `expectedResolvedPath` 绑定真实目标，链接重指向
 *    由后端按冲突拒绝。
 *
 * 运行：npm run check:file-manager
 */
import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";

import {
  entryKindOf,
  isDirectory,
  isRegularFile,
  isSymlink,
  isSpecial,
  canOpenAsDocument,
} from "../src/components/FileManager/entryKind.ts";

function fail(message) {
  console.error("File manager link contract failed: " + message);
  process.exitCode = 1;
}

const entry = entry_type => ({
  name: "x",
  path: "/x",
  entry_type,
  size: 0,
  accessed: null,
  modified: null,
  permissions: null,
});

// ── 1. 单一谓词矩阵 ──────────────────────────────────────────────
const matrix = [
  ["directory", true, false, false, false, false],
  ["file", false, true, false, false, true],
  ["symlink", false, false, true, false, false],
  ["fifo", false, false, false, true, false],
  ["socket", false, false, false, true, false],
  ["block_device", false, false, false, true, false],
  ["char_device", false, false, false, true, false],
  ["other", false, false, false, true, false],
];

for (const [type, dir, file, link, special, openable] of matrix) {
  const item = entry(type);
  const label = `kind matrix for ${type}`;
  assert.equal(entryKindOf(item), type, label);
  assert.equal(isDirectory(item), dir, label);
  assert.equal(isRegularFile(item), file, label);
  assert.equal(isSymlink(item), link, label);
  assert.equal(isSpecial(item), special, label);
  assert.equal(canOpenAsDocument(item), openable, label);
}

// ── 2. 前端不得保留第二份类型真源 ────────────────────────────────
const sourceRoot = "src";
const sourceFiles = [];
(function walk(directory) {
  for (const dirent of fs.readdirSync(directory, { withFileTypes: true })) {
    const full = path.join(directory, dirent.name);
    if (dirent.isDirectory()) walk(full);
    else if (/\.(ts|tsx)$/.test(dirent.name)) sourceFiles.push(full);
  }
})(sourceRoot);

for (const file of sourceFiles) {
  const source = fs.readFileSync(file, "utf8");
  if (/\.is_dir\b/.test(source)) fail(`${file} still uses the removed is_dir field`);
}

const typesSource = fs.readFileSync("src/components/FileManager/types.ts", "utf8");
if (/is_dir/.test(typesSource)) fail("SftpEntry must not declare is_dir");
if (/entry_type\?:/.test(typesSource)) fail("SftpEntry.entry_type must be required (single source of truth)");

// ── 3. 解析只发生在激活路径上 ────────────────────────────────────
const activationSource = fs.readFileSync(
  "src/components/FileManager/entryActivation.ts",
  "utf8",
);
if (!activationSource.includes("sftp_resolve_entry_cmd")) {
  fail("entryActivation.ts must own the sftp_resolve_entry_cmd call");
}
if (!activationSource.includes("specialFileCannotOpen")) {
  fail("entryActivation.ts must explain why a special file cannot be activated");
}
for (const file of sourceFiles) {
  if (file.endsWith("entryActivation.ts")) continue;
  if (fs.readFileSync(file, "utf8").includes("sftp_resolve_entry_cmd")) {
    fail(`${file} must not call sftp_resolve_entry_cmd outside entryActivation.ts`);
  }
}

const panelSource = fs.readFileSync("src/components/FileManager/FileManagerPanel.tsx", "utf8");
if (panelSource.includes("canOpenDocument")) fail("FileManagerPanel must consume entryKind helpers");
if (!panelSource.includes("activationInFlightRef")) {
  fail("FileManagerPanel must ignore repeated activation while a link resolution is in flight");
}

const documentDialogSource = fs.readFileSync(
  "src/components/FileEditor/RemoteDocumentDialog.tsx",
  "utf8",
);
if (!documentDialogSource.includes("resolvedPath")) {
  fail("RemoteDocumentDialog must show the document session's pinned resolvedPath");
}

// ── 4. 后端契约存在且注册 ─────────────────────────────────────────
const commandsSource = fs.readFileSync("src-tauri/src/plugins/ssh/commands.rs", "utf8");
for (const required of ["sftp_resolve_entry_cmd", "sftp_resolve_entry"]) {
  if (!commandsSource.includes(required)) fail(`commands.rs missing ${required}`);
}
const catalogSource = fs.readFileSync("src-tauri/src/plugins/catalog.rs", "utf8");
if (!catalogSource.includes("sftp_resolve_entry_cmd")) {
  fail("new command must be registered in the plugin command catalog");
}
const serviceSource = fs.readFileSync("src-tauri/src/plugins/ssh/sftp/service.rs", "utf8");
for (const required of ["pub async fn sftp_resolve_entry", "read_link", "canonicalize"]) {
  if (!serviceSource.includes(required)) fail(`service.rs missing resolver step ${required}`);
}
const attrsSource = fs.readFileSync("src-tauri/src/plugins/ssh/sftp/attrs.rs", "utf8");
if (!attrsSource.includes("pub struct ResolvedEntry")) fail("attrs.rs must own ResolvedEntry");
if (/pub is_dir: bool/.test(attrsSource)) fail("SftpEntry must not carry is_dir");

// ── 5. 远程文档绑定真实目标 ──────────────────────────────────────
const documentSource = fs.readFileSync("src-tauri/src/plugins/ssh/sftp/document.rs", "utf8");
for (const required of ["resolved_path", "expected_resolved_path", "save_document"]) {
  if (!documentSource.includes(required)) fail(`document.rs missing ${required}`);
}
const documentService = fs.readFileSync("src/services/remoteDocumentService.ts", "utf8");
for (const required of ["resolvedPath", "expectedResolvedPath"]) {
  if (!documentService.includes(required)) fail(`remoteDocumentService.ts missing ${required}`);
}

// ── 6. 链接相关文案双语对齐 ──────────────────────────────────────
const linkKeys = [
  "linkTarget",
  "linkTargetPath",
  "linkTargetStatus",
  "linkTargetMissing",
  "linkTargetPermissionDenied",
  "linkTargetUnsupported",
  "linkTargetTooDeep",
  "linkTargetSpecial",
  "linkTargetUnresolvable",
  "copyLinkTarget",
  "openLinkTarget",
  "deleteSymlinkConfirm",
  "confirmBatchDeleteWithSymlinks",
  "specialFileCannotOpen",
  "specialFileCannotDownload",
];
for (const locale of ["zh-CN", "en-US"]) {
  const messages = JSON.parse(
    fs.readFileSync(`src/i18n/locales/${locale}.json`, "utf8"),
  );
  const fileManagerKeys = messages.fileManager ?? {};
  for (const key of linkKeys) {
    if (typeof fileManagerKeys[key] !== "string") {
      fail(`${locale}.fileManager missing link i18n key ${key}`);
    }
  }
}

if (!process.exitCode) {
  console.log("File manager link contract passed.");
}
