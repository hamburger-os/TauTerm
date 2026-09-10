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
assert.match(hook, /dismissTask\(sessionId, transferId\)/);
assert.match(hook, /SUCCESS_AUTO_HIDE_MS = 5000/);
assert.match(
  hook,
  /window\.setTimeout[\s\S]{0,520}dismissTask\(sessionId, transferId\)/,
  "successful SFTP cards must remove their exact task snapshot when the five-second auto-hide fires",
);
assert.match(hook, /hoveredRef\.current/);
assert.match(hook, /autoHideDeadlineRef/);
assert.match(hook, /autoHideRemainingRef/);
assert.match(
  hook,
  /autoHideRemainingRef\.current = Math\.max\([\s\S]{0,180}autoHideDeadlineRef\.current - Date\.now\(\)/,
  "hovering a completed SFTP card must preserve the remaining auto-hide duration",
);
assert.match(
  hook,
  /scheduleAutoHide\(sftpTask\.transferId, autoHideRemainingRef\.current\)/,
  "leaving a completed SFTP card must resume the paused auto-hide duration",
);
assert.match(
  hook,
  /autoHideTransferIdRef\.current !== sftpTask\.transferId[\s\S]{0,520}hoveredRef\.current = false/,
  "a new transfer must not inherit hover state from a previously dismissed card",
);
assert.match(
  hook,
  /const hideProgress = useCallback[\s\S]{0,180}hoveredRef\.current = false/,
  "manually closing a completed card must clear its hover state",
);
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
assert.match(hook, /speed:\s*task\.speed/);
assert.match(hook, /completedAt:\s*task\.completedAt/);
assert.match(
  hook,
  /Date\.now\(\) - sftpTask\.completedAt >= SUCCESS_AUTO_HIDE_MS/,
  "remounting a FileManager must immediately discard an already-expired completed task instead of resurrecting its card",
);
assert.doesNotMatch(
  hook,
  /performance\.now\(\)/,
  "FileManager projection must not compute transfer throughput from WebView event timing",
);

// ── Narrow responsive status UI ────────────────────────────────────────────
const bar = await source("src/components/FileManager/TransferProgressBar.tsx");
assert.match(bar, /phase === "transferring"/);
assert.match(bar, /transferFinalizing/);
assert.match(bar, /transferCompleted/);
assert.match(
  bar,
  /case "completed":[\s\S]{0,220}formatSpeed\(speed\)/,
  "completed SFTP cards should retain and display the last reliable throughput sample",
);
assert.match(bar, /return "—"/);
assert.doesNotMatch(bar, /0 KB\/s/);
assert.match(
  bar,
  /if \(error && phase !== "completed" && phase !== "cancelled"\)[\s\S]{0,80}return error/,
  "active transfer/cancellation errors must be visible instead of tooltip-only",
);
assert.match(bar, /data-has-error=\{Boolean\(error\)/);

const barCss = await source("src/components/FileManager/TransferProgressBar.module.css");
assert.match(barCss, /grid-template-areas:\s*"name progress percent detail action"/);
assert.match(barCss, /\.closeBtn\s*\{[\s\S]*grid-area:\s*action/);
assert.match(barCss, /@container filemanager \(max-width: 360px\)/);
assert.match(barCss, /@container filemanager \(max-width: 220px\)/);
const narrow280 = barCss.slice(
  barCss.indexOf("@container filemanager (max-width: 280px)"),
  barCss.indexOf("@container filemanager (max-width: 220px)"),
);
assert.doesNotMatch(
  narrow280,
  /\.liveSpeed\s*\{[\s\S]*display:\s*none/,
  "live speed must remain visible in normal narrow sidebars; only the extreme <=220px tier may hide it",
);
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
assert.match(
  orchestrator,
  /write_tx\.send\(IoLoopCmd::HandoffPort[\s\S]{0,260}transfer_scheduler\.finish\(Some\(transfer_id\)\)[\s\S]{0,180}SessionState::Connected/,
  "failed Inline handoff dispatch must release Scheduler occupancy instead of waiting forever",
);
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
const rightSidebar = await source("src/components/RightSidebar/SessionRightSidebar.tsx");
assert.match(
  rightSidebar,
  /const FileManagerPanel = lazy\(\(\) => import\("\.\.\/FileManager\/FileManagerPanel"\)\)/,
  "SSH FileManager should remain lazy-loaded instead of inflating daily-driver ui-core",
);

const viteConfig = await source("vite.config.ts");
assert.match(viteConfig, /return "ui-file-manager"/);
assert.doesNotMatch(
  viteConfig,
  /components\\\/\(Common\|Layout\|Terminal\|RightSidebar\|JournaldViewer\|FileManager\|SendBar\)/,
  "FileManager must not be forced back into the ui-core manual chunk",
);

const panel = await source("src/components/FileManager/FileManagerPanel.tsx");
assert.match(panel, /useToast/);
assert.doesNotMatch(
  panel,
  /\balert\s*\(/,
  "FileManager must use the themed Toast path instead of native alert() UI",
);

const panelCss = await source("src/components/FileManager/FileManager.module.css");
assert.doesNotMatch(
  panelCss,
  /backdrop-filter\s*:/,
  "FileManager component CSS must not create private backdrop filters outside the global theme layer",
);
assert.doesNotMatch(
  panelCss,
  /var\(--glass-bg\)/,
  "FileManager must not reference the undefined --glass-bg theme token",
);
assert.match(
  panelCss,
  /\.dropOverlay[\s\S]{0,420}background:\s*var\(--control-surface\)/,
  "drag/drop overlay must reuse the shared themed control surface",
);
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
assert.match(panel, /getCurrentWebview/);
assert.match(panel, /onDragDropEvent/);
assert.match(panel, /getCurrentWindow\(\)[\s\S]*scaleFactor/);
assert.match(panel, /position\.x \/ scaleFactor/);
assert.match(panel, /handleDroppedPaths/);
assert.match(panel, /requestConflictPolicy\(conflictCount, false\)/);
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
assert.match(panel, /function canPreviewEntry\(entry: SftpEntry\)/);
assert.match(
  panel,
  /if \(canPreviewEntry\(ctxTarget\)\)[\s\S]{0,120}id: "preview"/,
  "ordinary files must expose the bounded byte preview regardless of filename extension",
);
assert.doesNotMatch(panel, /TEXT_EXTENSIONS|function isTextFile/);
assert.match(panel, /ms\.handleRightClick\(entry\);/);
assert.match(
  panel,
  /ctxOpenedRef\.current = true;[\s\S]{0,180}queueMicrotask\(\(\) => \{[\s\S]{0,80}ctxOpenedRef\.current = false/,
  "context-menu dedupe must expire after the current right-click instead of blocking later blank-area menus",
);
assert.match(
  panel,
  /if \(isConnected\) return;[\s\S]{0,220}resolveConflictPolicy\(null\)[\s\S]{0,180}closePreview\(\)/,
  "disconnect must close transient FileManager UI and resolve any pending conflict decision",
);
assert.match(
  panel,
  /const handleNewFile = useCallback\(\(\) => \{[\s\S]{0,180}if \(!isConnected\)[\s\S]{0,140}sessionDisconnected/,
  "new-file actions must fail closed while the SFTP session is disconnected",
);
assert.match(
  panel,
  /id: "upload"[\s\S]{0,100}disabled: !isConnected/,
  "disconnected blank-area context menus must disable upload actions",
);
assert.match(
  panel,
  /onClick=\{handleNewFile\}[\s\S]{0,100}disabled=\{!isConnected\}/,
  "disconnected toolbar mutation actions must be visibly disabled",
);
assert.match(
  panel,
  /\.catch\(\(error\) => \{[\s\S]{0,180}showToast\("error", String\(error\)\)/,
  "post-chmod metadata refresh failures must not be swallowed",
);
assert.match(panel, /const propsRequestGenerationRef = useRef\(0\)/);
assert.match(panel, /const previewRequestGenerationRef = useRef\(0\)/);
assert.match(
  panel,
  /generation !== propsRequestGenerationRef\.current[\s\S]{0,80}return/,
  "stale Properties stat responses must be ignored",
);
assert.match(
  panel,
  /generation !== previewRequestGenerationRef\.current[\s\S]{0,80}return/,
  "stale Preview byte responses must be ignored",
);
assert.match(
  panel,
  /const closeProperties = useCallback[\s\S]{0,160}propsRequestGenerationRef\.current \+= 1/,
  "closing Properties must invalidate its in-flight stat request",
);
assert.match(
  panel,
  /const closePreview = useCallback[\s\S]{0,160}previewRequestGenerationRef\.current \+= 1/,
  "closing Preview must invalidate its in-flight read request",
);
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
assert.match(sharedContext, /TASK_DISCARD/);
assert.match(sharedContext, /const dismissTask = useCallback/);
assert.match(
  sharedContext,
  /completedAt:\s*payload\.success \? Date\.now\(\) : null/,
  "successful unified tasks must record completion time for remount-safe five-second retention",
);
assert.match(sharedContext, /dispatch\(\{ type: "TASK_STARTED", payload \}\)/);
assert.match(sharedContext, /dispatch\(\{ type: "TASK_PROGRESS", payload: p \}\)/);
assert.match(sharedContext, /dispatch\(\{ type: "TASK_FINISHED", payload \}\)/);
assert.match(sharedContext, /const ack = await invoke<TransferStartAck>/);
assert.match(sharedContext, /activeTransferIdRef\.current = ack\.transfer_id/);
assert.match(sharedContext, /p\.transfer_id !== activeTransferIdRef\.current/);
assert.match(sharedContext, /batch_complete 只是协议层批次收尾[\s\S]*if \(p\.is_batch_complete\)/);
assert.match(sharedContext, /const cancelTask = useCallback/);
assert.match(sharedContext, /const measuredSpeed =/);
assert.match(
  sharedContext,
  /speed:\s*payload\.success \? current\.speed : null/,
  "successful completion must preserve the last reliable SFTP throughput sample for the auto-dismiss card",
);
assert.match(sharedContext, /TASK_CANCEL_REJECTED/);
assert.match(
  sharedContext,
  /dispatch\(\{ type: "TASK_CANCELLING", sessionId, transferId \}\)[\s\S]{0,320}await invoke\("file_transfer_cancel"/,
  "cancelling must be recorded before the cancel IPC to prevent terminal-state regression",
);
assert.match(
  sharedContext,
  /current\.phase === "cancelling"[\s\S]{0,220}phase = "cancelling"/,
  "late progress must not regress an accepted cancellation back to transferring",
);
assert.match(
  sharedContext,
  /TASK_DISCARD_SESSION"[\s\S]{0,220}event\.payload\.session_id !== activeSessionIdRef\.current/,
  "disconnect must discard per-session task snapshots even for FileManager-owned SFTP tasks",
);
assert.match(sharedContext, /file_transfer_cancel[\s\S]{0,180}transferId/);
assert.doesNotMatch(
  sharedContext,
  /file_transfer_cancel[\s\S]{0,260}SET_STATUS"[\s\S]{0,80}"cancelled"/,
  "cancel request acceptance must not be treated as terminal cancellation",
);

// FileManager delete confirmation delegates all shell/focus/button behavior to the shared dialog.
const deleteDialog = await source("src/components/FileManager/DeleteConfirmationDialog.tsx");
assert.match(deleteDialog, /ConfirmDialog/);
assert.match(deleteDialog, /open=\{message !== null\}/);
assert.match(deleteDialog, /intent="danger"/);
assert.match(deleteDialog, /size="compact"/);
assert.match(deleteDialog, /onConfirm=\{onConfirm\}/);
assert.match(deleteDialog, /onCancel=\{onCancel\}/);
assert.doesNotMatch(deleteDialog, /deleteConfirmAction/);

const confirmDialog = await source("src/components/common/ConfirmDialog.tsx");
assert.match(confirmDialog, /role="alertdialog"/);
assert.match(confirmDialog, /aria-modal="true"/);
assert.match(confirmDialog, /data-action="cancel"/);
assert.match(confirmDialog, /data-action="confirm"/);
assert.match(confirmDialog, /querySelector<HTMLButtonElement>\('\[data-action="cancel"\]:not\(:disabled\)'\)/);
assert.match(confirmDialog, /event\.key === "Escape"/);
assert.match(confirmDialog, /event\.key !== "Tab"/);
assert.match(confirmDialog, /querySelectorAll<HTMLElement>/);
assert.match(confirmDialog, /GlassButton/);
assert.match(confirmDialog, /variant="ghost"/);
assert.match(confirmDialog, /variant=\{intent\}/);
assert.match(confirmDialog, /size="md"/);
assert.match(confirmDialog, /t\("common\.cancel"\)/);
assert.match(confirmDialog, /t\("common\.confirm"\)/);
assert.match(confirmDialog, /useReducedMotion/);
assert.match(
  confirmDialog,
  /transition=\{\{ duration: reducedMotion \? 0 : 0\.12 \}\}/,
  "shared confirmation motion must respect the system reduced-motion preference",
);

const confirmDialogCss = await source("src/components/common/ConfirmDialog.module.css");
assert.match(confirmDialogCss, /border-radius:\s*var\(--radius-xl\)/);
assert.match(confirmDialogCss, /font-size:\s*var\(--text-md\)/);
assert.match(confirmDialogCss, /font-weight:\s*700/);
assert.match(confirmDialogCss, /font-size:\s*var\(--text-sm\)/);

const conflictDialog = await source("src/components/FileManager/ConflictResolutionModal.tsx");
assert.match(conflictDialog, /role="alertdialog"/);
assert.match(conflictDialog, /data-policy="keep-both"/);
assert.match(conflictDialog, /querySelector<HTMLButtonElement>\('\[data-policy="keep-both"\]'\)/);
assert.match(conflictDialog, /event\.key !== "Tab"/);
assert.match(conflictDialog, /dialogRef\.current\?\.querySelectorAll/);
assert.match(conflictDialog, /styles\.policyList/);
assert.match(conflictDialog, /styles\.footer/);
assert.match(conflictDialog, /variant="danger"/);
assert.doesNotMatch(
  conflictDialog,
  /variant="primary"/,
  "multi-choice conflict decisions must not promote a recommendation to a full Prism Primary button",
);
assert.match(
  conflictDialog,
  /variant="secondary"[\s\S]{0,120}data-policy="keep-both"/,
  "Keep Both must stay on the neutral secondary surface while retaining safe default focus",
);
assert.match(conflictDialog, /variant="ghost"/);
assert.match(conflictDialog, /useReducedMotion/);
assert.match(
  conflictDialog,
  /variant="ghost"[\s\S]{0,80}size="md"/,
  "dialog footer cancel action must use the standard md GlassButton geometry",
);

const conflictDialogCss = await source("src/components/FileManager/ConflictResolutionModal.module.css");
assert.match(conflictDialogCss, /border-radius:\s*var\(--radius-xl\)/);
assert.match(conflictDialogCss, /font-size:\s*var\(--text-md\)/);
assert.match(conflictDialogCss, /font-weight:\s*700/);
assert.match(conflictDialogCss, /font-size:\s*var\(--text-sm\)/);

const propertiesModal = await source("src/components/FileManager/FilePropertiesModal.tsx");
assert.match(propertiesModal, /role="dialog"/);
assert.match(propertiesModal, /aria-modal="true"/);
assert.match(propertiesModal, /aria-labelledby="file-properties-title"/);
assert.match(propertiesModal, /data-action="close"/);
assert.match(propertiesModal, /dialogRef\.current\?\.querySelectorAll/);
assert.match(propertiesModal, /event\.key !== "Tab"/);
assert.match(propertiesModal, /const canChmod = entryType === "file" \|\| entryType === "directory"/);
assert.match(propertiesModal, /\{canChmod && \(/);
assert.match(propertiesModal, /getEntryIcon/);
assert.match(
  propertiesModal,
  /setChmodValue\(getOctalFromPerms\(statInfo\?\.permissions \?\? null\)\)/,
  "Properties must clear stale chmod state when the next entry has no reported permissions",
);
assert.match(propertiesModal, /className=\{styles\.chmodEditor\}/);
assert.match(propertiesModal, /className=\{styles\.chmodError\} role="alert"/);
assert.match(
  propertiesModal,
  /if \(chmodEditingRef\.current\)[\s\S]{0,120}cancelChmodEditRef\.current\(\)[\s\S]{0,120}else[\s\S]{0,80}onClose\(\)/,
  "Escape must leave chmod editing before it closes the Properties dialog",
);
assert.match(
  propertiesModal,
  /\}, \[visible, onClose\]\);/,
  "changing chmod edit state must not rerun the modal focus-entry effect and steal input focus",
);
assert.match(propertiesModal, /perms\[3\] === "s" \|\| perms\[3\] === "S"/);
assert.match(propertiesModal, /perms\[6\] === "s" \|\| perms\[6\] === "S"/);
assert.match(propertiesModal, /perms\[9\] === "t" \|\| perms\[9\] === "T"/);
assert.match(
  propertiesModal,
  /\^\[0-7\]\{3,4\}\$/,
  "chmod editor must accept special-bit forms such as 4755",
);
assert.match(propertiesModal, /maxLength=\{4\}/);
assert.match(
  propertiesModal,
  /if \(!activeRef\.current\) return;[\s\S]{0,120}onChmodComplete\?\.\(\)/,
  "a chmod response arriving after the dialog closes must not restart Properties work",
);

assert.match(
  service,
  /file_type_bits = stat\.permissions\.unwrap_or\(0\) & 0o170000[\s\S]*mode & 0o7777/,
  "chmod must preserve POSIX file-type bits",
);
assert.match(
  service,
  /p & 0o4000 != 0[\s\S]{0,260}'s'[\s\S]{0,520}p & 0o2000 != 0[\s\S]{0,260}'s'[\s\S]{0,520}p & 0o1000 != 0[\s\S]{0,260}'t'/,
  "permission strings must preserve setuid, setgid and sticky bits for the chmod editor",
);
assert.match(service, /permission_strings_preserve_posix_special_bits/);
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
assert.match(fileList, /aria-sort=/);
assert.match(fileList, /className=\{styles\.errorBanner\} role="alert"/);
assert.match(fileList, /styles\.colPerms[^\n]*role="columnheader"/);
assert.match(fileList, /onClick=\{\(additiveKey, shiftKey\) => onEntryClick/);
assert.match(fileList, /className=\{styles\.gridFrame\}[\s\S]{0,80}role="grid"/);
assert.match(fileList, /className=\{styles\.header\} role="row"/);
assert.match(fileList, /className=\{styles\.body\}[\s\S]{0,60}role="presentation"/);
assert.match(
  fileList,
  /aria-rowcount=\{entries\.length \+ \(parentVisible \? 1 : 0\) \+ 1\}/,
  "list grid row count must include the column-header row",
);

const fileRow = await source("src/components/FileManager/FileRow.tsx");
assert.match(fileRow, /e\.ctrlKey \|\| e\.metaKey/);
assert.match(
  fileRow,
  /onClick\(e\.ctrlKey \|\| e\.metaKey, e\.shiftKey\)/,
  "keyboard Space must preserve additive/range selection modifiers",
);
assert.match(fileRow, /aria-rowindex=\{ariaRowIndex\}/);

const fileGrid = await source("src/components/FileManager/FileGrid.tsx");
assert.match(fileGrid, /VIRTUAL_THRESHOLD = 300/);
assert.match(fileGrid, /GRID_ROW_HEIGHT = 48/);
assert.match(fileGrid, /ResizeObserver/);
assert.match(fileGrid, /case "ArrowLeft"/);
assert.match(fileGrid, /case "ArrowRight"/);
assert.match(fileGrid, /case "ArrowUp"/);
assert.match(fileGrid, /case "ArrowDown"/);
assert.match(fileGrid, /tabIndex=\{activeItem === itemIndex \? 0 : -1\}/);
assert.match(fileGrid, /className=\{styles\.errorBanner\} role="alert"/);
assert.match(fileGrid, /e\.ctrlKey \|\| e\.metaKey/);
assert.match(fileGrid, /aria-rowindex=\{ariaRowIndex\}/);
assert.match(fileGrid, /aria-rowindex=\{itemIndex \+ 1\}/);

const multiSelect = await source("src/components/FileManager/hooks/useMultiSelect.ts");
assert.match(
  multiSelect,
  /if \(shiftKey && lastClickedIndex !== null\)[\s\S]{0,180}if \(!additiveKey\) next\.clear\(\)/,
  "plain Shift must replace selection with the anchor range while Ctrl/Command+Shift extends it",
);
assert.match(multiSelect, /handleRightClick: \(entry: SftpEntry\) => void/);
assert.match(multiSelect, /const \[lastClickedPath, setLastClickedPath\] = useState<string \| null>\(null\)/);
assert.match(
  multiSelect,
  /entries\.findIndex\(\(entry\) => entry\.path === lastClickedPath\)/,
  "range selection anchor must follow entry identity across sort/reload order changes",
);
assert.match(
  multiSelect,
  /const validPaths = new Set\(entries\.map\(\(entry\) => entry\.path\)\)[\s\S]{0,360}validPaths\.has\(path\)/,
  "refreshes must prune selected paths that no longer exist in the current directory",
);
assert.doesNotMatch(
  multiSelect,
  /handleRightClick[\s\S]{0,320}ctrlKey/,
  "context-menu selection must not treat macOS Control-click as an additive-selection modifier",
);

const breadcrumb = await source("src/components/FileManager/BreadcrumbNav.tsx");
assert.match(breadcrumb, /<nav className=\{styles\.breadcrumb\}/);
assert.match(breadcrumb, /aria-current="page"/);

const inlinePrompt = await source("src/components/FileManager/InlinePrompt.tsx");
assert.match(inlinePrompt, /GlassButton/);
assert.match(inlinePrompt, /aria-label=\{placeholder \?\? t\("fileManager\.name"\)\}/);

const contextMenu = await source("src/components/common/ContextMenu.tsx");
assert.match(contextMenu, /role="menu"/);
assert.match(contextMenu, /role="menuitem"/);
assert.match(contextMenu, /role="separator"/);
assert.match(contextMenu, /case "ArrowDown"/);
assert.match(contextMenu, /case "ArrowUp"/);
assert.match(contextMenu, /case "Home"/);
assert.match(contextMenu, /case "End"/);
assert.match(contextMenu, /useReducedMotion/);
assert.match(contextMenu, /const previousFocusRef = useRef<HTMLElement \| null>\(null\)/);
assert.match(
  contextMenu,
  /\}, \[state\.visible, state\.x, state\.y\]\);/,
  "menu positioning/focus work must rerun for a fresh context-click without depending on unstable state object identity",
);
assert.match(contextMenu, /if \(adjustedY < 0\) adjustedY = 8/);
assert.match(
  contextMenu,
  /querySelector<HTMLButtonElement>\('button\[role="menuitem"\]:not\(:disabled\)'\)/,
  "context menus must move focus to the first enabled action when opened",
);

const preview = await source("src/components/FileManager/FilePreviewModal.tsx");
assert.match(preview, /role="dialog"/);
assert.match(preview, /aria-modal="true"/);
assert.match(preview, /data-action="close"/);
assert.match(preview, /dialogRef\.current\?\.querySelectorAll/);
assert.match(preview, /event\.key !== "Tab"/);
assert.match(preview, /type PreviewEncoding/);
assert.match(preview, /"gb18030"/);
assert.match(preview, /"shift_jis"/);
assert.match(preview, /function formatHex/);
assert.match(preview, /HEX_RENDER_LIMIT/);
assert.match(preview, /new TextDecoder\(encoding/);
assert.match(
  preview,
  /detected === "utf-16le" \|\| detected === "utf-16be"[\s\S]{0,80}\? "text"/,
  "BOM-detected UTF-16 files must default to Text instead of being misclassified by NUL bytes",
);
assert.match(preview, /className=\{styles\.error\} role="alert"/);
assert.match(preview, /Math\.min\(bytes\.length, HEX_RENDER_LIMIT\)/);
assert.match(preview, /aria-pressed=\{mode === "text"\}/);
assert.match(preview, /liquid-selector-strip/);
assert.match(preview, /liquid-selector-button/);
assert.doesNotMatch(preview, /modeButtonActive/);
assert.match(
  preview,
  /encodingSelect\} liquid-glass-input liquid-glass-select/,
  "preview encoding must use the canonical themed select rather than applying a surface class directly to native select",
);

const globalCss = await source("src/styles/global.css");
const canonicalSelectBlock = globalCss.slice(
  globalCss.indexOf(".liquid-glass-select {"),
  globalCss.indexOf(".liquid-glass-select option"),
);
assert.match(
  canonicalSelectBlock,
  /color-scheme:\s*dark/,
  "dark-theme native select popups must advertise a dark color scheme",
);
assert.match(
  globalCss,
  /\[data-theme="frosted"\] \.liquid-glass-select[\s\S]{0,100}color-scheme:\s*light/,
  "Frosted native select popups must advertise the light color scheme",
);
assert.ok(
  canonicalSelectBlock.indexOf("padding: var(--select-padding)") <
    canonicalSelectBlock.indexOf("padding-right: 26px"),
  "select arrow-safe right padding must be declared after the shorthand so it is not reset",
);
