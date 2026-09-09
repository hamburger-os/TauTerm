import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");

async function source(relativePath) {
  return readFile(path.join(ROOT, relativePath), "utf8");
}

const hook = await source("src/components/FileManager/hooks/useSftpProgress.ts");
assert.match(hook, /'preparing'[\s\S]*'transferring'[\s\S]*'finalizing'[\s\S]*'cancelling'[\s\S]*'completed'[\s\S]*'failed'[\s\S]*'cancelled'/);
assert.match(hook, /payload\.transfer_id !== activeTransferIdRef\.current/);
assert.match(hook, /payload\.bytes_per_second/);
assert.doesNotMatch(
  hook,
  /Date\.now\(\)|performance\.now\(\)/,
  "SFTP speed must come from the backend I/O layer, not WebView event arrival timing",
);
assert.match(hook, /SUCCESS_AUTO_HIDE_MS = 5000/);
assert.match(hook, /hoveredRef\.current/);
assert.match(hook, /phase = hasKnownTotal && payload\.bytes_done >= payload\.bytes_total[\s\S]*'finalizing'/);
assert.match(
  hook,
  /cancelTransfer[\s\S]*previousPhase[\s\S]*phase: previousPhase/,
  "a cancel-command failure must not falsely terminate a still-running transfer",
);

const bar = await source("src/components/FileManager/TransferProgressBar.tsx");
assert.match(bar, /phase === "transferring"/);
assert.match(bar, /transferFinalizing/);
assert.match(bar, /transferCompleted/);
assert.match(bar, /transferFailed/);
assert.match(bar, /return "—"/);
assert.doesNotMatch(bar, /0 KB\/s/);

const barCss = await source("src/components/FileManager/TransferProgressBar.module.css");
assert.match(barCss, /grid-template-areas:\s*"name progress percent detail action"/);
assert.match(barCss, /\.closeBtn\s*\{[\s\S]*grid-area:\s*action/);
assert.match(barCss, /@container filemanager \(max-width: 360px\)/);
assert.match(barCss, /@container filemanager \(max-width: 280px\)/);
assert.match(barCss, /@container filemanager \(max-width: 220px\)/);
assert.doesNotMatch(
  barCss,
  /overflow-x\s*:\s*(auto|scroll)/,
  "transfer actions must remain reachable without horizontal scrolling",
);

for (const relativePath of [
  "src/components/FileManager/FileList.module.css",
  "src/components/FileManager/FileGrid.module.css",
]) {
  const css = await source(relativePath);
  assert.match(css, /padding-bottom:\s*42px/);
  assert.match(css, /max-width: 360px[\s\S]*padding-bottom:\s*52px/);
  assert.match(css, /max-width: 280px[\s\S]*padding-bottom:\s*56px/);
  assert.match(css, /max-width: 220px[\s\S]*padding-bottom:\s*44px/);
}

const eventTypes = await source("src/components/FileManager/types.ts");
assert.match(eventTypes, /transfer_id:\s*string/);
assert.match(eventTypes, /bytes_per_second:\s*number \| null/);

const unified = await source("src-tauri/src/kernel/file_transfer.rs");
assert.match(unified, /pub transfer_id:\s*String/);
assert.match(unified, /pub bytes_per_second:\s*Option<f64>/);
assert.match(unified, /pub fn chunk_with_speed/);

const service = await source("src-tauri/src/transfer/ssh_file_service.rs");
assert.match(service, /struct TransferRateEstimator/);
assert.match(service, /Instant::now\(\)/);
assert.match(service, /fn should_emit_final/);
assert.match(service, /if throttle\.should_emit_final\(total, local_size\)/);
assert.match(service, /if throttle\.should_emit_final\(total, remote_size\)/);
assert.doesNotMatch(
  service,
  /\/\/ 最终进度事件（确保 UI 显示 100%）[\s\S]{0,120}cb\(total,/,
  "SFTP must not unconditionally emit a duplicate terminal 100% sample",
);

const sftp = await source("src-tauri/src/transfer/sftp_transfer.rs");
assert.match(sftp, /UnifiedProgress::chunk_with_speed/);
assert.match(sftp, /if skipped > 0 \|\| cancel\.load\(Ordering::SeqCst\)/);
assert.match(
  sftp,
  /if failed > 0[\s\S]{0,500}return Err\(FileTransferError::Other/,
  "partial SFTP batch failure must propagate as a failed transfer",
);

const orchestrator = await source("src-tauri/src/transfer/orchestrator.rs");
assert.match(orchestrator, /uuid::Uuid::new_v4\(\)\.to_string\(\)/);
assert.match(orchestrator, /progress\.transfer_id = transfer_id\.clone\(\)/);
assert.match(
  orchestrator,
  /drop\(progress_tx\);[\s\S]{0,120}broadcaster\.await/,
  "completion must wait until queued progress events are drained",
);
assert.match(
  orchestrator,
  /guard\.complete\(\);[\s\S]{0,220}file-transfer:finished/,
  "SFTP session transfer occupancy must be released before finished is emitted",
);

const fileManager = await source("src/components/FileManager/hooks/useFileManager.ts");
assert.match(fileManager, /async function runSftpTransferAndWait/);
assert.match(fileManager, /payload\.transfer_id !== activeTransferId/);
assert.match(fileManager, /await runSftpTransferAndWait\(sessionId/);

console.log("file-transfer-lifecycle: responsive actions, state machine, identity, ordering, speed, and batch semantics verified");
