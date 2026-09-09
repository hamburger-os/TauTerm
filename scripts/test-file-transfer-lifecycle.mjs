import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const source = (relativePath) => readFile(path.join(ROOT, relativePath), "utf8");

// ── Compact SFTP status lifecycle ──────────────────────────────────────────
const hook = await source("src/components/FileManager/hooks/useSftpProgress.ts");
assert.match(hook, /'preparing'[\s\S]*'transferring'[\s\S]*'finalizing'[\s\S]*'cancelling'[\s\S]*'completed'[\s\S]*'failed'[\s\S]*'cancelled'/);
assert.match(hook, /payload\.transfer_id !== activeTransferIdRef\.current/);
assert.match(hook, /payload\.bytes_per_second/);
assert.doesNotMatch(hook, /Date\.now\(\)|performance\.now\(\)/);
assert.match(hook, /SUCCESS_AUTO_HIDE_MS = 5000/);
assert.match(hook, /hoveredRef\.current/);
assert.match(hook, /payloadComplete && isLastFile \? 'finalizing' : 'transferring'/);
assert.match(
  hook,
  /file_transfer_cancel[\s\S]{0,180}transferId:\s*activeTransferIdRef\.current/,
  "SFTP cancellation must target the exact transfer id",
);
assert.match(
  hook,
  /previousPhase[\s\S]*phase: previousPhase/,
  "cancel-command failure must restore the running phase",
);

// ── Narrow responsive status UI ────────────────────────────────────────────
const bar = await source("src/components/FileManager/TransferProgressBar.tsx");
assert.match(bar, /phase === "transferring"/);
assert.match(bar, /transferFinalizing/);
assert.match(bar, /transferCompleted/);
assert.match(bar, /return "—"/);
assert.doesNotMatch(bar, /0 KB\/s/);

const barCss = await source("src/components/FileManager/TransferProgressBar.module.css");
assert.match(barCss, /grid-template-areas:\s*"name progress percent detail action"/);
assert.match(barCss, /\.closeBtn\s*\{[\s\S]*grid-area:\s*action/);
assert.match(barCss, /@container filemanager \(max-width: 360px\)/);
assert.match(barCss, /@container filemanager \(max-width: 220px\)/);
assert.doesNotMatch(barCss, /overflow-x\s*:\s*(auto|scroll)/);

// ── Unified task identity and options ──────────────────────────────────────
const unified = await source("src-tauri/src/kernel/file_transfer.rs");
assert.match(unified, /pub transfer_id:\s*String/);
assert.match(unified, /pub bytes_per_second:\s*Option<f64>/);
assert.match(unified, /pub enum OverwritePolicy[\s\S]*Replace[\s\S]*Skip[\s\S]*KeepBoth/);
assert.match(unified, /pub struct FileTransferOptions[\s\S]*destination_paths:\s*Vec<String>/);

const commands = await source("src-tauri/src/commands.rs");
assert.match(commands, /file_transfer_send[\s\S]{0,260}TransferStartAck/);
assert.match(commands, /file_transfer_receive[\s\S]{0,260}TransferStartAck/);
assert.match(commands, /destination_paths:\s*Option<Vec<String>>/);
assert.match(commands, /overwrite_policy:\s*Option<String>/);
assert.match(commands, /file_transfer_cancel[\s\S]{0,260}transfer_id:\s*Option<String>/);
assert.match(commands, /max_bytes\s*=\s*max_bytes\.min\(1_048_576\)/);

const orchestrator = await source("src-tauri/src/transfer/orchestrator.rs");
assert.match(orchestrator, /pub struct TransferStartAck[\s\S]*transfer_id:\s*String/);
assert.match(orchestrator, /store\.transfer_start\(&internal_id, &transfer_id\)/);
assert.match(orchestrator, /progress\.transfer_id = transfer_id\.clone\(\)/);
assert.match(orchestrator, /drop\(progress_tx\);[\s\S]{0,120}broadcaster\.await/);
assert.match(orchestrator, /guard\.complete\(\);[\s\S]{0,220}file-transfer:finished/);

const sessionStore = await source("src-tauri/src/kernel/session_store.rs");
assert.match(sessionStore, /pub active_transfer_id:\s*Option<String>/);
assert.match(sessionStore, /active_transfer_id\.as_deref\(\) != Some\(expected\)/);
assert.match(sessionStore, /没有正在进行的侧通道传输/);

// ── Transactional SFTP writes ──────────────────────────────────────────────
const service = await source("src-tauri/src/transfer/ssh_file_service.rs");
assert.match(service, /enum SftpWriteOutcome/);
assert.match(service, /struct SftpUploadOptions[\s\S]*overwrite_policy:\s*OverwritePolicy/);
assert.match(
  service,
  /OpenFlags::WRITE\s*\|\s*OpenFlags::CREATE\s*\|\s*OpenFlags::EXCLUDE/,
  "new remote objects and transfer temp files must use exclusive creation",
);
assert.match(
  service,
  /try_commit_local_noreplace[\s\S]*hard_link\(temp, candidate\)/,
  "local KeepBoth/Skip commit must reserve the final name without overwrite",
);
assert.match(service, /try_commit_remote_noreplace/);
assert.match(
  service,
  /读取本地文件失败[\s\S]{0,480}remove_file\(&temp_path\)/,
  "upload local-read failures must clean the already-created remote temp file",
);
assert.match(service, /sibling_local_artifact\(&final_path, "part"\)/);
assert.match(service, /remote_sibling_artifact\(&final_path, "part"\)/);
assert.match(service, /sibling_local_artifact\(final_path, "backup"\)/);
assert.match(service, /remote_sibling_artifact\(final_path, "backup"\)/);
assert.match(
  service,
  /commit_local_temp[\s\S]*rename\(final_path, &backup\)[\s\S]*rename\(temp, final_path\)[\s\S]*rename\(&backup, final_path\)/,
  "local replace must be backup + commit + rollback, never direct truncate of final path",
);
assert.match(
  service,
  /commit_remote_temp[\s\S]*rename\(final_path, &backup\)[\s\S]*rename\(temp, final_path\)[\s\S]*rename\(&backup, final_path\)/,
  "remote replace must be backup + commit + rollback",
);
assert.match(
  service,
  /sftp_download[\s\S]*sibling_local_artifact\(&final_path, "part"\)[\s\S]*File::create\(&temp_path\)/,
);
assert.doesNotMatch(
  service,
  /File::create\(local_path\)/,
  "downloads must never truncate the final destination before commit",
);
assert.match(
  service,
  /sftp_upload[\s\S]*remote_sibling_artifact\(&final_path, "part"\)[\s\S]*open_with_flags[\s\S]*OpenFlags::EXCLUDE/,
);
assert.doesNotMatch(
  service,
  /sftp\.create\(remote_path\)/,
  "uploads must never truncate the final remote destination before commit",
);
assert.doesNotMatch(
  service,
  /\.create\(remote_path\)/,
  "New File must never use the truncating create helper on an existing path",
);
assert.match(service, /symlink_metadata\(remote_path\)/);
assert.match(service, /pub enum SftpEntryType[\s\S]*Symlink/);
assert.match(service, /pub async fn sftp_list_tree_recursive/);
assert.match(service, /SftpEntryType::Directory[\s\S]*result\.push\(child\)/);
assert.match(
  service,
  /sftp_read_head[\s\S]*symlink_metadata[\s\S]*SftpEntryType::File/,
  "preview must reject non-regular entries instead of following links",
);

// ── SFTP adapter: explicit plans, empty directories and no-follow links ─────
const sftp = await source("src-tauri/src/transfer/sftp_transfer.rs");
assert.match(sftp, /struct ReceiveFilePlan/);
assert.match(sftp, /options\s*\.destination_paths/);
assert.match(sftp, /prepare_local_directory_destination/);
assert.match(
  sftp,
  /OverwritePolicy::KeepBoth[\s\S]*try_create_local_directory/,
  "directory KeepBoth must reserve a distinct root instead of merging into an existing folder",
);
assert.match(sftp, /sftp_list_tree_recursive/);
assert.match(sftp, /SftpEntryType::Symlink[\s\S]*符号链接默认不跟随/);
assert.match(sftp, /options\s*\.overwrite_policy/);
assert.match(sftp, /目录替换不会自动合并或递归覆盖/);
assert.match(sftp, /if failed > 0[\s\S]{0,500}FileTransferError::Other/);
assert.doesNotMatch(
  sftp,
  /cleanup_remote_partial/,
  "adapter must not delete a final remote path after transactional upload failure",
);

// ── File manager wait/query correctness and Save As ────────────────────────
const fileManager = await source("src/components/FileManager/hooks/useFileManager.ts");
assert.match(fileManager, /async function runSftpTransferAndWait/);
assert.match(fileManager, /invoke<TransferStartAck>\('file_transfer_receive'/);
assert.match(fileManager, /const ack = await startTransfer\(\)[\s\S]*activeTransferId = ack\.transfer_id/);
assert.match(fileManager, /bufferedFinished = new Map<string, TransferFinishedPayload>/);
assert.doesNotMatch(fileManager, /TRANSFER_TIMEOUT_MS|timed out after 5 minutes/);
assert.match(fileManager, /destinationPaths = \[localPath as string\]/);
assert.match(fileManager, /directoryGenerationRef\.current/);
assert.match(
  fileManager,
  /generation !== directoryGenerationRef\.current[\s\S]*return/,
  "stale directory queries must not commit their results",
);

// ── Standard file-manager interaction rules ────────────────────────────────
const panel = await source("src/components/FileManager/FileManagerPanel.tsx");
assert.doesNotMatch(panel, /window\.confirm\(/, "file deletion must use the themed confirmation dialog");
assert.match(panel, /DeleteConfirmationDialog/);
assert.match(panel, /deleteConfirmMessage/);
assert.match(panel, /pendingDeleteTargets/);
assert.match(panel, /deleteDirConfirm/);
assert.match(panel, /confirmBatchDelete/);
assert.match(
  panel,
  /setPendingDeleteTargets\(\[\.\.\.targets\]\)[\s\S]{0,120}setDeleteConfirmMessage\(message\)/,
  "delete confirmation must snapshot the exact targets before asking for approval",
);
assert.match(panel, /requestConflictPolicy/);
assert.match(panel, /conflictCount/);
assert.match(panel, /name === "\." \|\| name === "\.\."/);
assert.match(panel, /name\.includes\("\/"\)/);

// Properties must be the final actionable menu item for both directory and file menus.
const directoryMenu = panel.match(/if \(ctxTarget\.is_dir\) \{[\s\S]*?return \[([\s\S]*?)\];/)?.[1] ?? "";
assert.ok(directoryMenu, "directory menu must exist");
assert.ok(
  directoryMenu.lastIndexOf('id: "properties"') > directoryMenu.lastIndexOf('id: "delete"'),
  "directory Properties must come after Delete",
);
const fileMenuStart = panel.indexOf("// File");
const multiMenuStart = panel.indexOf("// Multi-select");
const fileMenu = panel.slice(fileMenuStart, multiMenuStart);
assert.ok(
  fileMenu.lastIndexOf('id: "properties"') > fileMenu.lastIndexOf('id: "delete"'),
  "file Properties must be the last action",
);

// ── Shared transfer state obeys finished as the only terminal source ───────
const sharedContext = await source("src/context/TransferContext.tsx");
assert.match(sharedContext, /const ack = await invoke<TransferStartAck>/);
assert.match(sharedContext, /activeTransferIdRef\.current = ack\.transfer_id/);
assert.match(sharedContext, /p\.transfer_id !== activeTransferIdRef\.current/);
assert.match(sharedContext, /batch_complete 只是协议层批次收尾[\s\S]*if \(p\.is_batch_complete\)/);
assert.match(
  sharedContext,
  /file_transfer_cancel[\s\S]{0,180}transferId:\s*activeTransferIdRef\.current/,
);
assert.doesNotMatch(
  sharedContext,
  /file_transfer_cancel[\s\S]{0,220}SET_STATUS"[\s\S]{0,80}"cancelled"/,
  "cancel request acceptance must not be treated as terminal cancellation",
);

console.log("file-transfer-lifecycle: transactional writes, exact identity, no-follow traversal, query ordering, responsive UI, and interaction contracts verified");

const deleteDialog = await source("src/components/FileManager/DeleteConfirmationDialog.tsx");
assert.match(deleteDialog, /role="alertdialog"/);
assert.match(deleteDialog, /requestAnimationFrame\(\(\) => cancelRef\.current\?\.focus\(\)\)/);
assert.match(deleteDialog, /event\.key === "Escape"/);
assert.match(deleteDialog, /event\.key === "Tab"/);
assert.match(deleteDialog, /deleteConfirmAction/);

const propertiesModal = await source("src/components/FileManager/FilePropertiesModal.tsx");
assert.match(propertiesModal, /const canChmod = entryType === "file" \|\| entryType === "directory"/);
assert.match(propertiesModal, /\{canChmod && \(/);

assert.match(
  service,
  /file_type_bits = stat\.permissions\.unwrap_or\(0\) & 0o170000[\s\S]*mode & 0o7777/,
  "chmod must preserve POSIX file-type bits",
);
assert.match(
  service,
  /仅支持修改普通文件或目录权限/,
  "chmod must reject symlink/special-file targets",
);
