import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const source = (relativePath) => readFile(path.join(ROOT, relativePath), "utf8");

// ── One frontend task owner ─────────────────────────────────────────────────
const context = await source("src/context/TransferContext.tsx");
assert.match(context, /tasksById:\s*Record<string, ManagedTransferTask>/);
assert.match(context, /taskIdsBySession:\s*Record<string, string\[\]>/);
assert.match(context, /startErrorsBySession:\s*Record<string, string>/);
assert.match(context, /TASK_STARTED/);
assert.match(context, /TASK_PROGRESS/);
assert.match(context, /TASK_FINISHED/);
assert.match(context, /TASK_CANCELLING/);
assert.match(context, /TASK_CANCEL_REJECTED/);
assert.match(context, /getTaskForSession/);
assert.match(context, /getActiveTaskForSession/);
assert.match(context, /isManagedTransferTerminalPhase/);
assert.match(context, /completedAt:\s*payload\.success \? Date\.now\(\) : null/);
assert.match(context, /speed:\s*payload\.success \? current\.speed : null/);
assert.match(
  context,
  /dispatch\(\{ type: "TASK_CANCELLING", sessionId, transferId \}\)[\s\S]{0,260}cancelFileTransfer\(sessionId, transferId\)/,
  "cancel must enter cancelling before exact transfer-id IPC",
);
assert.match(
  context,
  /current\.phase === "cancelling"[\s\S]{0,180}phase = "cancelling"/,
  "late progress must not regress cancelling state",
);
assert.match(
  context,
  /resultProjection\(payload\)[\s\S]{0,500}payload\.results/,
  "finished results must be able to replace provisional per-file progress with exact terminal results",
);
assert.doesNotMatch(context, /tasksBySession/);
assert.doesNotMatch(context, /activeProtocolRef|activeSessionIdRef|activeTransferIdRef/);
assert.doesNotMatch(context, /backendStartedRef|lastAggregateBytesRef/);
assert.doesNotMatch(
  context,
  /const \[state, dispatch\][\s\S]*\bactiveProtocol:\s*|\bactiveSessionId:\s*/,
  "frontend must not keep a second global active-transfer owner",
);

// ── Shared frontend command/wait service ────────────────────────────────────
const transferService = await source("src/services/transferService.ts");
assert.match(transferService, /export async function startFileTransfer\(/);
assert.match(transferService, /export async function cancelFileTransfer\(/);
assert.match(transferService, /export async function startFileTransferAndWait\(/);
assert.match(transferService, /file_transfer_send/);
assert.match(transferService, /file_transfer_receive/);
assert.match(transferService, /file_transfer_cancel/);
assert.match(transferService, /bufferedFinished = new Map<string, TransferFinishedPayload>/);
assert.match(
  transferService,
  /unlisten = await listen<TransferFinishedPayload>[\s\S]*const ack = await startFileTransfer/,
  "finished listener must be registered before start to cover tiny-transfer races",
);
assert.match(transferService, /payload\.transfer_id !== activeTransferId/);
assert.doesNotMatch(transferService, /setTimeout|TRANSFER_TIMEOUT_MS/);

// ── FileManager uses the service/store instead of owning a second lifecycle ─
const fileManager = await source("src/components/FileManager/hooks/useFileManager.ts");
assert.match(fileManager, /startFileTransfer\('send'/);
assert.match(fileManager, /startFileTransferAndWait\(/);
assert.match(fileManager, /getTaskForSession\(sessionId\)/);
assert.match(fileManager, /isManagedTransferTerminalPhase\(transferTask\.phase\)/);
assert.match(fileManager, /refreshedTransferIdRef\.current === transferTask\.transferId/);
assert.match(fileManager, /destinationPaths = \[localPath as string\]/);
assert.match(fileManager, /directoryGenerationRef\.current/);
assert.doesNotMatch(fileManager, /async function runSftpTransferAndWait/);
assert.doesNotMatch(fileManager, /file-transfer:started|file-transfer:progress|file-transfer:finished/);
assert.doesNotMatch(fileManager, /invoke<TransferStartAck>\('file_transfer_(send|receive)'/);

// ── Compact SFTP projection ─────────────────────────────────────────────────
const hook = await source("src/components/FileManager/hooks/useSftpProgress.ts");
assert.match(hook, /getTaskForSession\(sessionId\)/);
assert.match(hook, /cancelTask\(sessionId, sftpTask\.transferId\)/);
assert.match(hook, /dismissTask\(sessionId, transferId\)/);
assert.match(hook, /SUCCESS_AUTO_HIDE_MS = 5000/);
assert.match(hook, /speed:\s*task\.speed/);
assert.match(hook, /completedAt:\s*task\.completedAt/);
assert.match(hook, /Date\.now\(\) - sftpTask\.completedAt >= SUCCESS_AUTO_HIDE_MS/);
assert.match(hook, /hoveredRef\.current/);
assert.match(hook, /autoHideRemainingRef/);
assert.doesNotMatch(hook, /listen<|file-transfer:started|file-transfer:progress|file-transfer:finished/);
assert.doesNotMatch(hook, /invoke\(/);
assert.doesNotMatch(hook, /performance\.now\(\)/);

// ── Session-scoped Transmission panel ──────────────────────────────────────
const transmission = await source("src/components/Transmission/TransmissionPanel.tsx");
assert.match(transmission, /getTaskForSession\(sessionId\)/);
assert.match(transmission, /getActiveTaskForSession\(sessionId\)/);
assert.match(transmission, /state\.startErrorsBySession\[sessionId\]/);
assert.match(transmission, /clearError\(sessionId\)/);
assert.match(transmission, /task\?\.files \?\? \[\]/);
assert.doesNotMatch(transmission, /state\.activeSessionId|state\.activeProtocol/);

// ── Task identity and protocol-independent backend contract ─────────────────
const unified = await source("src-tauri/src/kernel/file_transfer.rs");
assert.match(unified, /pub transfer_id:\s*String/);
assert.match(unified, /pub bytes_per_second:\s*Option<f64>/);
assert.match(unified, /pub enum OverwritePolicy[\s\S]*Replace[\s\S]*Skip[\s\S]*KeepBoth/);
assert.match(unified, /pub struct FileTransferOptions[\s\S]*destination_paths:\s*Vec<String>/);
assert.match(unified, /file_success:\s*Some\(files_failed == 0\)/);
assert.doesNotMatch(
  unified,
  /files_failed > 0 \|\| files_skipped > 0/,
  "Skip is a user policy result, not a transport failure",
);

const protocol = await source("src-tauri/src/transfer/protocol.rs");
assert.match(protocol, /pub trait SerialTransferProtocol/);
assert.match(protocol, /Option<Box<dyn SerialTransferProtocol>>/);
assert.match(protocol, /真正跨传输方式的扩展点是 `kernel::file_transfer::FileTransfer`/);

const transferMod = await source("src-tauri/src/transfer/mod.rs");
assert.doesNotMatch(transferMod, /pub mod manager;/);
await assert.rejects(
  source("src-tauri/src/transfer/manager.rs"),
  /ENOENT/,
  "legacy TransferManager must be removed so strategy resolution has one owner",
);

// ── Scheduler model is extensible without enabling unsafe concurrency ───────
const scheduler = await source("src-tauri/src/transfer/scheduler.rs");
assert.match(scheduler, /DEFAULT_MAX_ACTIVE_PER_SESSION:\s*usize = 1/);
assert.match(scheduler, /active:\s*HashMap<String, ScheduledTransfer>/);
assert.match(scheduler, /pub fn with_max_active/);
assert.match(scheduler, /pub fn active_count/);
assert.match(scheduler, /self\.active\.len\(\) >= self\.max_active/);
assert.match(scheduler, /存在多个传输任务，请指定 transfer_id/);
assert.match(scheduler, /fn has_inline_transfer/);
assert.match(scheduler, /bounded_map_model_supports_future_side_channel_concurrency/);
assert.match(scheduler, /inline_is_exclusive_even_when_side_channel_limit_is_higher/);
assert.match(scheduler, /side_channel_cannot_start_while_inline_owns_session_io/);

const sessionStore = await source("src-tauri/src/kernel/session_store.rs");
assert.match(sessionStore, /pub transfer_scheduler:\s*TransferScheduler/);
assert.match(sessionStore, /pub transfer_tasks:\s*Vec<tokio::task::JoinHandle<\(\)>>/);
assert.match(sessionStore, /reserve_inline_transfer/);
assert.match(sessionStore, /reserve_side_channel/);
assert.match(sessionStore, /cancel_scheduled_transfer/);
assert.match(sessionStore, /register_transfer_task/);
assert.doesNotMatch(sessionStore, /pub active_transfer_id:|pub transfer_cancel:|pub cancel_transfer_tx:/);

// ── All strategies now use one start/ack/event lifecycle ───────────────────
const orchestrator = await source("src-tauri/src/transfer/orchestrator.rs");
assert.match(orchestrator, /pub struct TransferStartAck[\s\S]*transfer_id:\s*String/);
assert.match(orchestrator, /store\.transfer_start\(&internal_id, &transfer_id\)/);
assert.match(orchestrator, /reserve_inline_transfer/);
assert.match(orchestrator, /progress\.transfer_id = transfer_id\.clone\(\)/);
assert.match(orchestrator, /fn emit_transfer_started/);
assert.match(orchestrator, /fn emit_transfer_finished/);
assert.match(orchestrator, /"results": results/);
assert.match(
  orchestrator,
  /InlineTransferOrchestrator[\s\S]*oneshot::channel::<\(\)>\(\)[\s\S]*tokio::spawn[\s\S]*register_transfer_task[\s\S]*emit_transfer_started[\s\S]*start_tx\.send\(\(\)\)[\s\S]*Ok\(ack\)/,
  "Inline transfer start must register a background task, publish started, open its gate and immediately return ack",
);
assert.match(
  orchestrator,
  /SideChannelTransferOrchestrator[\s\S]*PanicGuard::new[\s\S]*if start_rx\.await\.is_err\(\)[\s\S]*register_transfer_task/,
  "SideChannel gate failure must still be guarded so Scheduler occupancy cannot leak",
);
assert.match(
  orchestrator,
  /drop\(progress_tx\);[\s\S]{0,120}broadcaster\.await/,
  "finished must not overtake queued progress events",
);
assert.match(
  orchestrator,
  /return_port[\s\S]*emit_transfer_finished/,
  "Inline resources must be returned before terminal event is emitted",
);
assert.match(
  orchestrator,
  /handle\.state != SessionState::Disconnected[\s\S]{0,120}handle\.state = SessionState::Connected/,
  "Inline cleanup must never resurrect a Session that was disconnected while a background transfer was finishing",
);
assert.doesNotMatch(orchestrator, /emit_transfer_failed/);
assert.doesNotMatch(orchestrator, /active_transfer_id|cancel_transfer_tx/);

// ── Transactional SFTP commit and mid-transfer source mutation guard ─────────
const service = await source("src-tauri/src/transfer/ssh_file_service.rs");
assert.match(service, /enum SftpWriteOutcome/);
assert.match(service, /OpenFlags::WRITE\s*\|\s*OpenFlags::CREATE\s*\|\s*OpenFlags::EXCLUDE/);
assert.match(service, /try_commit_local_noreplace[\s\S]*hard_link\(temp, candidate\)/);
assert.match(service, /try_commit_remote_noreplace/);
assert.match(service, /sibling_local_artifact\(&final_path, "part"\)/);
assert.match(service, /remote_sibling_artifact\(&final_path, "part"\)/);
assert.match(service, /sibling_local_artifact\(final_path, "backup"\)/);
assert.match(service, /remote_sibling_artifact\(final_path, "backup"\)/);
assert.match(
  service,
  /commit_local_temp[\s\S]*rename\(final_path, &backup\)[\s\S]*rename\(temp, final_path\)[\s\S]*rename\(&backup, final_path\)/,
  "local Replace must retain rollback capability until new temp is committed",
);
assert.match(
  service,
  /commit_remote_temp[\s\S]*rename\(final_path, &backup\)[\s\S]*rename\(temp, final_path\)[\s\S]*rename\(&backup, final_path\)/,
  "remote Replace must retain rollback capability until new temp is committed",
);
assert.doesNotMatch(service, /File::create\(local_path\)/);
assert.doesNotMatch(service, /sftp\.create\(remote_path\)/);
assert.match(
  service,
  /let \(mut remote_file, remote_size, remote_mtime\)[\s\S]*total != remote_size \|\| !remote_unchanged[\s\S]*remove_file\(&temp_path\)[\s\S]*未提交正式目标/,
  "download must verify the remote source snapshot before committing its local temp file",
);
assert.match(
  service,
  /let initial_meta = local_file[\s\S]*local_modified[\s\S]*total != local_size \|\| !local_unchanged[\s\S]*remove_file\(&temp_path\)[\s\S]*未提交正式目标/,
  "upload must verify the local source snapshot before committing its remote temp file",
);
assert.match(service, /meta\.size\.unwrap_or\(0\) == remote_size/);
assert.match(service, /remote_mtime\.is_none\(\) \|\| meta\.mtime == remote_mtime/);
assert.match(service, /same_modified_time\(local_modified, meta\.modified\(\)\.ok\(\)\)/);
assert.match(service, /pub enum SftpEntryType[\s\S]*Symlink/);
assert.match(service, /pub async fn sftp_list_tree_recursive/);
assert.match(service, /pub async fn sftp_prepare_upload_directory/);
assert.match(service, /pub async fn sftp_ensure_directory/);
assert.match(service, /mode & 0o7777/);

// ── Existing SFTP adapter safety invariants remain intact ───────────────────
const sftp = await source("src-tauri/src/transfer/sftp_transfer.rs");
assert.match(sftp, /struct ReceiveFilePlan/);
assert.match(sftp, /struct LocalDirectoryScan/);
assert.match(sftp, /async fn scan_local_directory/);
assert.match(sftp, /meta\.file_type\(\)\.is_symlink\(\)[\s\S]*本地符号链接默认不跟随/);
assert.match(sftp, /sftp_prepare_upload_directory/);
assert.match(sftp, /sftp_ensure_directory/);
assert.match(sftp, /prepare_local_directory_destination/);
assert.match(sftp, /safe_local_relative/);
assert.match(sftp, /SftpEntryType::Symlink[\s\S]*符号链接默认不跟随/);
assert.match(sftp, /目录替换不会自动合并或递归覆盖/);
assert.doesNotMatch(sftp, /cleanup_remote_partial/);

// ── Narrow transfer status UI keeps speed/action reachable ──────────────────
const bar = await source("src/components/FileManager/TransferProgressBar.tsx");
assert.match(bar, /phase === "transferring"/);
assert.match(bar, /transferFinalizing/);
assert.match(bar, /transferCompleted/);
assert.match(bar, /case "completed":[\s\S]{0,220}formatSpeed\(speed\)/);
assert.match(bar, /return "—"/);
assert.doesNotMatch(bar, /0 KB\/s/);

const barCss = await source("src/components/FileManager/TransferProgressBar.module.css");
assert.match(barCss, /grid-template-areas:\s*"name progress percent detail action"/);
assert.match(barCss, /@container filemanager \(max-width: 360px\)/);
assert.match(barCss, /@container filemanager \(max-width: 220px\)/);
assert.doesNotMatch(barCss, /overflow-x\s*:\s*(auto|scroll)/);

console.log("file-transfer lifecycle contract: ok");
