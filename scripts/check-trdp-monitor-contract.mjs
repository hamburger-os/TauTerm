import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import path from "node:path";

const ROOT = process.cwd();
const read = relative => readFile(path.join(ROOT, relative), "utf8");

const [router, monitorView, connectForm] = await Promise.all([
  read("src/plugins/trdp/TrdpSessionRouter.tsx"),
  read("src/plugins/trdp/TrdpMonitorView.tsx"),
  read("src/plugins/trdp/TrdpConnectForm.tsx"),
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

for (const field of [
  "capture_interface",
  "capture_interface_b_enabled",
  "capture_interface_b",
  "capture_filter_auto",
  "capture_filter",
]) {
  assert.match(
    connectForm,
    new RegExp(`"${field}"`),
    `TRDP Monitor persistent configuration must own ${field}`,
  );
}

console.log("trdp-monitor: passive routing, navigation and persisted capture configuration contracts preserved");
