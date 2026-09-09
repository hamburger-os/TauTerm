import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const source = (relativePath) => readFile(path.join(ROOT, relativePath), "utf8");

// ── Compact SFTP status projection ──────────────────────────────────────────
const hook = await source("src/components/FileManager/hooks/useSftpProgress.ts");
assert.match(hook, /useTransfer/);
assert.match(hook, /state\.tasksBySession\[sessionId\]/);
assert.match(hook, /cancelTask\(sessionId, sftpTask\.transferId\)/);
assert.match(hook, /SUCCESS_AUTO_HIDE_MS = 5000/);
assert.match(hook, /hoveredRef\.current/);
assert.doesNotMatch(
  hook,
  /listen<|file-transfer:started|file-transfer:progress|file-transfer:finished/,
  "FileManager compact progress must project the unified TransferContext store, not own backend listeners",
);
assert.doesNotMatch(
  hook,
  /invoke\(/,
  "FileManager compact progress must not own a second transfer command path",
);
assert.doesNotMatch(hook, /Date\.now\(\)|performance\.now\(\)/);

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
assert.match(
  orchestrator,
  /oneshot::channel::<\(\)>\(\)[\s\S]*register_transfer_task[\s\S]*file-transfer:started[\s\S]*start_tx\.send\(\(\)\)/,
  "SideChannel tasks must be registered before started/progress can run",
);
assert.match(orchestrator, /reserve_inline_transfer/);
assert.match(orchestrator, /cancel_scheduled_transfer/);
assert.doesNotMatch(orchestrator, /cancel_transfer_tx|active_transfer_id|transfer_cancel/);

const sessionStore = await source("src-tauri/src/kernel/session_store.rs");
assert.match(sessionStore, /pub transfer_scheduler:\s*TransferScheduler/);
assert.match(sessionStore, /reserve_inline_transfer/);
assert.match(sessionStore, /reserve_side_channel/);
assert.match(sessionStore, /cancel_scheduled_transfer/);
assert.doesNotMatch(sessionStore, /pub active_transfer_id:|pub transfer_cancel:|pub cancel_transfer_tx:/);

const scheduler = await source("src-tauri/src/transfer/scheduler.rs");
assert.match(scheduler, /DEFAULT_MAX_ACTIVE_PER_SESSION:\s*usize = 1/);
assert.match(scheduler, /enum TransferCancelSignal[\s\S]*Inline[\s\S]*SideChannel/);
assert.match(scheduler, /pub fn reserve_inline/);
assert.match(scheduler, /pub fn reserve_side_channel/);
assert.match(scheduler, /pub fn cancel\(/);
assert.match(scheduler, /pub fn finish\(/);

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
assert.match(service, /pub async fn sftp_prepare_upload_directory/);
assert.match(service, /pub async fn sftp_ensure_directory/);
assert.match(
  service,
  /OverwritePolicy::KeepBoth[\s\S]*try_create_remote_directory/,
  "remote directory KeepBoth must reserve a distinct root without merging",
);
assert.match(service, /SftpEntryType::Directory[\s\S]*result\.push\(child\)/);
assert.match(
  service,
  /sftp_read_head[\s\S]*symlink_metadata[\s\S]*SftpEntryType::File/,
  "preview must reject non-regular entries instead of following links",
);

// ── SFTP adapter: explicit plans, empty directories and no-follow links ─────
const sftp = await source("src-tauri/src/transfer/sftp_transfer.rs");
assert.match(sftp, /struct ReceiveFilePlan/);
assert.match(sftp, /struct LocalDirectoryScan/);
assert.match(sftp, /async fn scan_local_directory/);
assert.match(sftp, /meta\.file_type\(\)\.is_symlink\(\)[\s\S]*本地符号链接默认不跟随/);
assert.match(sftp, /sftp_prepare_upload_directory/);
assert.match(sftp, /sftp_ensure_directory/);
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
assert.match(sftp, /fn local_safe_component/);
assert.match(sftp, /name\.contains\('\\\\'\)/);
assert.match(sftp, /name == "\.\."|name == '\.\.'/);
assert.match(sftp, /safe_local_relative/);
assert.match(
  sftp,
  /safe_local_relative[\s\S]*relative\.split\('\/'\)[\s\S]*local_safe_component/,
  "remote tree paths must be validated component-by-component before local join",
);
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
const directoryDownloadSection = fileManager.slice(
  fileManager.indexOf("// ── Download directory"),
  fileManager.indexOf("// ── SFTP 传输结束后刷新当前目录"),
);
assert.doesNotMatch(
  directoryDownloadSection,
  /destinationPaths:/,
  "directory roots must be derived and validated by the backend, not concatenated from remote names in WebView",
);
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
assert.match(panel, /handleUploadFolder/);
assert.match(panel, /fileManager\.uploadFolder/);
assert.match(
  panel,
  /requestConflictPolicy\(1, false\)/,
  "directory conflicts must not offer unsafe recursive Replace semantics",
);
assert.match(
  panel,
  /let overwritePolicy: OverwritePolicy = "keep-both"/,
  "an unseen/stale upload conflict must default to no-clobber",
);
assert.match(panel, /fileManager\.deleteFailed/);
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

// ── Unified transfer event store / terminal ownership ───────────────────────
const sharedContext = await source("src/context/TransferContext.tsx");
assert.match(sharedContext, /tasksBySession:\s*Record<string, ManagedTransferTask>/);
assert.match(sharedContext, /TASK_STARTED/);
assert.match(sharedContext, /TASK_PROGRESS/);
assert.match(sharedContext, /TASK_FINISHED/);
assert.match(sharedContext, /dispatch\(\{ type: "TASK_STARTED", payload \}\)/);
assert.match(sharedContext, /dispatch\(\{ type: "TASK_PROGRESS", payload: p \}\)/);
assert.match(sharedContext, /dispatch\(\{ type: "TASK_FINISHED", payload \}\)/);
assert.match(sharedContext, /const ack = await invoke<TransferStartAck>/);
assert.match(sharedContext, /activeTransferIdRef\.current = ack\.transfer_id/);
assert.match(sharedContext, /p\.transfer_id !== activeTransferIdRef\.current/);
assert.match(sharedContext, /batch_complete 只是协议层批次收尾[\s\S]*if \(p\.is_batch_complete\)/);
assert.match(sharedContext, /const cancelTask = useCallback/);
assert.match(sharedContext, /file_transfer_cancel[\s\S]{0,180}transferId/);
assert.doesNotMatch(
  sharedContext,
  /file_transfer_cancel[\s\S]{0,260}SET_STATUS"[\s\S]{0,80}"cancelled"/,
  "cancel request acceptance must not be treated as terminal cancellation",
);

const deleteDialog = await source("src/components/FileManager/DeleteConfirmationDialog.tsx");
assert.match(deleteDialog, /role="alertdialog"/);
assert.match(deleteDialog, /requestAnimationFrame\(\(\) => cancelRef\.current\?\.focus\(\)\)/);
assert.match(deleteDialog, /event\.key === "Escape"/);
assert.match(deleteDialog, /event\.key === "Tab"/);
assert.match(deleteDialog, /deleteConfirmAction/);

const conflictDialog = await source("src/components/FileManager/ConflictResolutionModal.tsx");
assert.match(conflictDialog, /keepBothRef\.current\?\.focus\(\)/);
assert.match(conflictDialog, /event\.key === "Tab"/);
assert.match(conflictDialog, /dialogRef\.current\?\.querySelectorAll/);

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


const transferTypes = await source("src-tauri/src/transfer/types.rs");
assert.match(transferTypes, /pub is_dir:\s*bool/);
assert.match(transferTypes, /symlink_metadata\(path\)/);
assert.match(transferTypes, /file_type\(\)\.is_symlink\(\)/);

const virtualWindow = await source("src/components/FileManager/hooks/useVirtualWindow.ts");
assert.match(virtualWindow, /threshold = 300/);
assert.match(virtualWindow, /ResizeObserver/);
assert.match(virtualWindow, /overscan/);

const fileList = await source("src/components/FileManager/FileList.tsx");
assert.match(fileList, /useVirtualWindow/);
assert.match(fileList, /tabIndex=\{activeIndex === index \? 0 : -1\}/);
assert.match(fileList, /case "ArrowDown"/);
assert.match(fileList, /case "Home"/);
assert.match(fileList, /case "End"/);
assert.match(fileList, /virtualCanvas/);

const fileGrid = await source("src/components/FileManager/FileGrid.tsx");
assert.match(fileGrid, /VIRTUAL_THRESHOLD = 300/);
assert.match(fileGrid, /ResizeObserver/);
assert.match(fileGrid, /case "ArrowLeft"/);
assert.match(fileGrid, /case "ArrowRight"/);
assert.match(fileGrid, /case "ArrowUp"/);
assert.match(fileGrid, /case "ArrowDown"/);
assert.match(fileGrid, /tabIndex=\{activeItem === itemIndex \? 0 : -1\}/);

const preview = await source("src/components/FileManager/FilePreviewModal.tsx");
assert.match(preview, /type PreviewEncoding/);
assert.match(preview, /"gb18030"/);
assert.match(preview, /"shift_jis"/);
assert.match(preview, /function formatHex/);
assert.match(preview, /HEX_RENDER_LIMIT/);
assert.match(preview, /new TextDecoder\(encoding/);
assert.match(preview, /aria-pressed=\{mode === "text"\}/);
