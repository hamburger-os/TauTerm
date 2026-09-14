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
  /current\.phase === "cancelling"[\s\S]{0,220}phase = "cancelling"/,
  "late progress must not regress cancelling state",
);
assert.match(
  context,
  /function resultProjection\(payload: TransferFinishedPayload\)[\s\S]{0,300}payload\.results\.map/,
  "finished payload results must be projected into exact per-file terminal entries",
);
assert.match(
  context,
  /const exactResults = resultProjection\(payload\);/,
  "TASK_FINISHED must consume exact backend results",
);
assert.match(
  context,
  /const files = exactResults \?\? current\.files\.map/,
  "TASK_FINISHED must prefer exact backend results over provisional progress state",
);
assert.match(context, /payload\.kind === "file_start"/);
assert.match(context, /payload\.kind === "file_complete"/);
assert.match(context, /payload\.kind === "batch_complete"/);
assert.doesNotMatch(context, /__batch_complete__/);
assert.doesNotMatch(context, /payload\.is_file_start|payload\.is_file_complete|payload\.is_batch_complete/);
assert.doesNotMatch(context, /tasksBySession/);
assert.doesNotMatch(context, /activeProtocolRef|activeSessionIdRef|activeTransferIdRef/);
assert.doesNotMatch(context, /backendStartedRef|lastAggregateBytesRef/);
assert.doesNotMatch(
  context,
  /const \[state, dispatch\][\s\S]*\bactiveProtocol:\s*|\bactiveSessionId:\s*/,
  "frontend must not keep a second global active-transfer owner",
);

const frontendTypes = await source("src/types/transfer.ts");
assert.match(frontendTypes, /export type TransferProgressKind[\s\S]*"file_start"[\s\S]*"progress"[\s\S]*"file_complete"[\s\S]*"batch_complete"/);
assert.match(frontendTypes, /interface UnifiedTransferProgressPayload[\s\S]*kind:\s*TransferProgressKind/);
assert.match(frontendTypes, /interface TransferFinishedPayload[\s\S]*transfer_id:\s*string[\s\S]*protocol:\s*string[\s\S]*cancelled:\s*boolean[\s\S]*results:\s*BatchFileResult\[\] \| null/);
assert.doesNotMatch(frontendTypes, /is_file_start|is_file_complete|is_batch_complete|__batch_complete__/);
assert.match(frontendTypes, /export type SendProtocolOptions[\s\S]*protocol: "ymodem"[\s\S]*protocol: "xmodem"[\s\S]*protocol: "zmodem"[\s\S]*protocol: "sftp"/);
assert.match(frontendTypes, /export type ReceiveProtocolOptions[\s\S]*checkMode: XmodemReceiveCheckMode[\s\S]*crcCapability: ZmodemReceiveCrcCapability/);
assert.match(frontendTypes, /interface FileTransferSendRequest[\s\S]*protocolOptions: SendProtocolOptions/);
assert.match(frontendTypes, /interface FileTransferReceiveRequest[\s\S]*protocolOptions: ReceiveProtocolOptions/);
assert.doesNotMatch(frontendTypes, /FileTransferSendRequest[\s\S]{0,260}\n  protocol: string/);
assert.doesNotMatch(frontendTypes, /FileTransferReceiveRequest[\s\S]{0,300}\n  blockSize\?/);

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
assert.doesNotMatch(hook, /__batch_complete__/);
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
assert.match(transmission, /<ProtocolConfigForm config=\{config\} onChange=\{setConfig\}/);
assert.match(context, /protocolOptions: sendProtocolOptions\(config\)/);
assert.match(context, /protocolOptions: receiveProtocolOptions\(config\)/);
assert.doesNotMatch(context, /request\.blockSize/);

// ── Task identity and protocol-independent backend contract ─────────────────
const unified = await source("src-tauri/src/kernel/file_transfer.rs");
assert.match(unified, /pub transfer_id:\s*String/);
assert.match(unified, /pub bytes_per_second:\s*Option<f64>/);
assert.match(unified, /pub enum TransferProgressKind[\s\S]*FileStart[\s\S]*Progress[\s\S]*FileComplete[\s\S]*BatchComplete/);
assert.match(unified, /pub kind:\s*TransferProgressKind/);
assert.match(unified, /pub enum OverwritePolicy[\s\S]*Replace[\s\S]*Skip[\s\S]*KeepBoth/);
assert.match(unified, /pub struct FileTransferOptions[\s\S]*destination_paths:\s*Vec<String>/);
assert.match(unified, /file_success:\s*Some\(files_failed == 0\)/);
assert.doesNotMatch(unified, /is_file_start|is_file_complete|is_batch_complete|__batch_complete__/);
assert.doesNotMatch(
  unified,
  /files_failed > 0 \|\| files_skipped > 0/,
  "Skip is a user policy result, not a transport failure",
);

const protocol = await source("src-tauri/src/transfer/protocol.rs");
assert.match(protocol, /pub trait SerialTransferProtocol/);
assert.doesNotMatch(protocol, /create_protocol\(/, "role-aware construction must not regress to a generic protocol factory");
assert.match(protocol, /真正跨传输方式的扩展点是 `kernel::file_transfer::FileTransfer`/);

const roleConfig = await source("src-tauri/src/transfer/config.rs");
assert.match(roleConfig, /pub enum SendProtocolOptions[\s\S]*Ymodem[\s\S]*Xmodem[\s\S]*Zmodem[\s\S]*Sftp/);
assert.match(roleConfig, /pub enum ReceiveProtocolOptions[\s\S]*Ymodem[\s\S]*Xmodem[\s\S]*Zmodem[\s\S]*Sftp/);
assert.match(roleConfig, /pub enum XModemReceiveMode[\s\S]*Auto[\s\S]*Crc16[\s\S]*Checksum/);
assert.match(roleConfig, /pub enum ZModemCrcPolicy[\s\S]*Crc32Required/);

const xmodem = await source("src-tauri/src/transfer/xmodem.rs");
assert.doesNotMatch(xmodem, /\bg_mode\b|const G:\s*u8|XModemVariant|OneK/);
assert.match(xmodem, /`G` 不代表 XMODEM-1K/);
assert.match(xmodem, /XModemCheckMode::Checksum[\s\S]*packet\.push\(crc::checksum\(data\)\)/);
assert.match(xmodem, /Some\(CAN\) => return Err\("发送方取消了传输"\.into\(\)\)/);
assert.match(xmodem, /fn next_block_num\(current: u8\) -> u8 \{\s*current\.wrapping_add\(1\)\s*\}/);
assert.match(xmodem, /bnum != !bnum_neg \|\| bnum != 1/);
assert.doesNotMatch(xmodem, /flush_port_buffer\(port\)/);
assert.match(xmodem, /Sender \{ block_size: usize \}/);
assert.match(xmodem, /Receiver \{ check_mode: XModemReceiveMode \}/);

const zmodem = await source("src-tauri/src/transfer/zmodem.rs");
assert.match(zmodem, /ZModemRole[\s\S]*Sender[\s\S]*crc_policy: ZModemCrcPolicy[\s\S]*Receiver[\s\S]*crc_capability: ZModemReceiveCrcCapability/);
assert.match(zmodem, /receiver_crc32[\s\S]*ZModemCrcPolicy::Auto => receiver_crc32/);
assert.match(zmodem, /ZModemCrcPolicy::Crc32Required if receiver_crc32 => true/);
assert.match(zmodem, /rinit_flags\[ZF0\] = if use_crc32 \{ CANFC32 \} else \{ 0 \}/);

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
assert.match(scheduler, /bounded_map_model_supports_future_auxiliary_concurrency/);
assert.match(scheduler, /inline_is_exclusive_even_when_auxiliary_limit_is_higher/);
assert.match(scheduler, /auxiliary_cannot_start_while_inline_owns_session_io/);

const sessionStore = await source("src-tauri/src/kernel/session_store.rs");
assert.match(sessionStore, /pub transfer_scheduler:\s*TransferScheduler/);
assert.match(sessionStore, /pub transfer_tasks:\s*Vec<tokio::task::JoinHandle<\(\)>>/);
assert.match(sessionStore, /reserve_inline_transfer/);
assert.match(sessionStore, /reserve_auxiliary/);
assert.match(sessionStore, /cancel_scheduled_transfer/);
assert.match(sessionStore, /register_transfer_task/);
assert.doesNotMatch(sessionStore, /pub active_transfer_id:|pub transfer_cancel:|pub cancel_transfer_tx:/);
assert.doesNotMatch(sessionStore, /reserve_inline_transfer[\s\S]{0,260}oneshot::Sender/);
assert.match(sessionStore, /reserve_inline_transfer[\s\S]{0,260}Arc<std::sync::atomic::AtomicBool>/);

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
  /AuxiliaryTransferOrchestrator[\s\S]*PanicGuard::new[\s\S]*if start_rx\.await\.is_err\(\)[\s\S]*register_transfer_task/,
  "Auxiliary gate failure must still be guarded so Scheduler occupancy cannot leak",
);
assert.match(
  orchestrator,
  /drop\(progress_tx\);[\s\S]{0,120}broadcaster\.await/,
  "finished must not overtake queued progress events",
);
assert.match(
  orchestrator,
  /drop\(transfer\);[\s\S]*restore_session_state\(&task_app, &task_sid, &task_transfer_id\);[\s\S]*emit_transfer_finished/,
  "Inline ExclusiveIo must be released and Session state restored before terminal event is emitted",
);
for (const legacyToken of ["HandoffPort", "channel_return_tx", "try_handoff", "return_port("]) {
  if (orchestrator.includes(legacyToken)) {
    throw new Error(`legacy inline-transfer handoff token must be removed: ${legacyToken}`);
  }
}
assert.match(
  orchestrator,
  /handle\.state != SessionState::Disconnected[\s\S]{0,120}handle\.state = SessionState::Connected/,
  "Inline cleanup must never resurrect a Session that was disconnected while a background transfer was finishing",
);
assert.doesNotMatch(orchestrator, /emit_transfer_failed/);
assert.doesNotMatch(orchestrator, /active_transfer_id|cancel_transfer_tx/);

// ── Inline transfers own the physical generic driver + unread bytes ─────────
const transportRuntime = await source("src-tauri/src/transport/runtime.rs");
assert.match(transportRuntime, /driver:\s*Option<Box<dyn BlockingByteStream>>/);
assert.match(transportRuntime, /struct ExclusiveLeasePayload[\s\S]*driver:\s*Box<dyn BlockingByteStream>[\s\S]*prefetched:\s*VecDeque<u8>/);
assert.match(
  transportRuntime,
  /AcquireExclusive[\s\S]{0,260}driver_tx:\s*mpsc::SyncSender<Result<ExclusiveLeasePayload, TransportError>>/,
  "exclusive acquisition must move the generic physical driver and unread bytes out of the actor",
);
assert.match(
  transportRuntime,
  /ReturnExclusive[\s\S]{0,220}driver:\s*Box<dyn BlockingByteStream>[\s\S]{0,80}prefetched:\s*VecDeque<u8>/,
  "exclusive release must return the same generic driver and remaining unread bytes to the actor",
);
assert.match(transportRuntime, /handoff_buffer:\s*VecDeque<u8>/);
assert.match(transportRuntime, /fn take_unconsumed_bytes/);
assert.match(transportRuntime, /fn restore_unconsumed_bytes/);
assert.match(
  transportRuntime,
  /impl Read for ExclusiveIo[\s\S]{0,420}self\.prefetched[\s\S]{0,520}driver_mut\(\)\?[\s\S]{0,40}\.read\(buf\)/,
  "exclusive protocol reads must consume prefetched handoff bytes before direct driver reads",
);
assert.match(
  transportRuntime,
  /impl Write for ExclusiveIo[\s\S]{0,300}driver_mut\(\)\?[\s\S]{0,80}\.write_all\(buf\)/,
  "exclusive protocol writes must execute directly against the leased driver",
);
assert.match(transportRuntime, /exclusive_handoff_preserves_bytes_read_during_acquisition/);
assert.match(transportRuntime, /unread_handoff_bytes_return_to_shared_subscriber/);
assert.match(transportRuntime, /exclusive_driver_runs_on_owner_thread_and_returns_to_actor_thread/);
assert.match(transportRuntime, /exclusive_io_error_does_not_close_shared_runtime/);
assert.doesNotMatch(transportRuntime, /ExclusiveRead|purge_input/);

const serialTransfer = await source("src-tauri/src/transfer/serial_transfer.rs");
assert.doesNotMatch(
  serialTransfer,
  /flush_port_buffer/,
  "inline adapter must not purge handshake bytes after acquiring the exclusive lease",
);

const serialTransport = await source("src-tauri/src/transport/serial.rs");
assert.doesNotMatch(serialTransport, /purge_input|bytes_to_read|serial_drain_input/);

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
