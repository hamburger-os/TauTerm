import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import path from "node:path";

const ROOT = process.cwd();
const read = relative => readFile(path.join(ROOT, relative), "utf8");

const [router, monitorView, connectForm, plugin, model, runtime] = await Promise.all([
  read("src/plugins/trdp/TrdpSessionRouter.tsx"),
  read("src/plugins/trdp/TrdpMonitorView.tsx"),
  read("src/plugins/trdp/TrdpConnectForm.tsx"),
  read("src/plugins/trdp/index.tsx"),
  read("src/plugins/trdp/model.ts"),
  read("src-tauri/src/plugins/trdp.rs"),
]);

assert.match(
  router,
  /mode === "monitor"[\s\S]*<TrdpMonitorView sessionId=\{sessionId\} \/>/,
  "TRDP Monitor sessions must route to the dedicated passive Monitor workspace",
);

for (const page of ["overview", "pd", "md", "analysis"]) {
  assert.match(
    monitorView,
    new RegExp(`\\["${page}", "trdp\\.nav\\.${page}"\\]`),
    `TRDP Monitor must expose the ${page} navigation page`,
  );
}

assert.doesNotMatch(
  monitorView,
  /object_start|object_update|object_stop|md_confirm|addPublisher|addSubscriber|addPdRequest|addMdRequest|addMdListener|addMdNotify/,
  "TRDP Monitor must remain passive and must not expose Node object/runtime send commands",
);
assert.match(
  monitorView,
  /canConfirmMessage=\{false\}/,
  "TRDP Monitor must never grant MD Confirm capability to captured traffic",
);

for (const field of ["capture_interfaces", "capture_filter_auto", "capture_filter"]) {
  assert.match(
    connectForm,
    new RegExp(field),
    `TRDP Monitor persistent configuration must own ${field}`,
  );
}

assert.match(
  model,
  /CaptureInterfaceRef[\s\S]*deviceName[\s\S]*displayName/,
  "TRDP capture configuration must separate the native device identifier from the display name",
);
assert.match(
  monitorView,
  /interface:\s*captureInterfaceA\.deviceName/,
  "TRDP live capture must pass the native device identifier to the runtime",
);
assert.match(
  plugin,
  /return capture\.a\?\.displayName \|\| unconfigured/,
  "TRDP Monitor sidebar subtitle must show only the human-readable interface name for one-link capture",
);
assert.doesNotMatch(
  plugin,
  /trdpSidebar\.capture/,
  "TRDP Monitor sidebar subtitle must not prefix the interface with a capture label",
);

assert.match(
  runtime,
  /capture_running:\s*Arc<AtomicBool>/,
  "TRDP runtime must own live-capture running state independently from React views",
);
assert.match(
  runtime,
  /operation == "runtime_state"[\s\S]*"capture"[\s\S]*"running"/,
  "TRDP runtime_state must expose the owned capture id and running state for view hydration",
);
assert.match(
  runtime,
  /operation == "capture_release"/,
  "TRDP must release owned live captures in Rust instead of forwarding an unsupported native command",
);
assert.match(
  runtime,
  /capture_running\.store\(true, Ordering::Release\)/,
  "TRDP runtime must mark a capture running only after capture_start succeeds",
);
assert.match(
  runtime,
  /capture_running\.store\(false, Ordering::Release\)/,
  "TRDP runtime must clear running state when capture stops or the runtime exits",
);

console.log("trdp-monitor: passive routing, capture identity, runtime ownership and persisted Monitor configuration contracts preserved");
