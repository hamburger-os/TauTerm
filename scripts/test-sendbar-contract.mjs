import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, resolve } from "node:path";
import {
  buildSendPayload,
  isHexInputValid,
  normalizeHexInput,
} from "../src/components/SendBar/sendPayload.ts";
import {
  parseAutoReplyConfig,
  parseCommandConfig,
  parseScriptImport,
  uniqueAssetName,
} from "../src/components/SendBar/assetValidation.ts";
import {
  canSyncNetworkSendTarget,
  isNetworkSendTargetVisible,
} from "../src/plugins/network/send-target.ts";
import { hasUsableRttDownChannel } from "../src/plugins/rtt/model.ts";
import {
  clampSendBarBodyHeight,
  getSendBarHostHeightCss,
  getSendBarHostMinHeightCss,
} from "../src/components/SendBar/sendBarLayout.ts";

const here = dirname(fileURLToPath(import.meta.url));
const root = resolve(here, "..");
const source = path => readFileSync(resolve(root, path), "utf8");

// Payload encoding is a single contract shared by manual and repeat sends.
assert.equal(buildSendPayload("ping", "text", "crlf"), "ping\r\n");
assert.equal(buildSendPayload("ping", "text", "lf"), "ping\n");
assert.equal(buildSendPayload("ping", "text", "cr"), "ping\r");
assert.equal(buildSendPayload("ping", "text", "none"), "ping");
assert.equal(buildSendPayload("   ", "text", "crlf"), null);
assert.equal(normalizeHexInput("AA 01\nff"), "AA01ff");
assert.equal(isHexInputValid("AA 01 ff"), true);
assert.equal(isHexInputValid("A"), false);
assert.equal(isHexInputValid("GG"), false);
assert.deepEqual([...buildSendPayload("AA 01 ff", "hex", "none")], [0xaa, 0x01, 0xff]);
assert.equal(buildSendPayload("AA 0", "hex", "none"), null);

// Importers accept only the current canonical schema. No legacy compatibility layer is allowed.
const commandSet = parseCommandConfig({
  version: 1,
  name: "Smoke",
  defaultDelay: 100,
  commands: [{ id: "1", command: "AT", note: "", delay: 50 }],
});
assert.equal(commandSet.commands[0].command, "AT");
assert.throws(() => parseCommandConfig({ version: 0, name: "legacy", defaultDelay: 0, commands: [] }));
assert.throws(() => parseCommandConfig({ version: 1, name: "bad", defaultDelay: -1, commands: [] }));
assert.throws(() => parseCommandConfig({
  version: 1,
  name: "dupe",
  defaultDelay: 0,
  commands: [
    { id: "same", command: "AT", note: "", delay: 0 },
    { id: "same", command: "ATI", note: "", delay: 0 },
  ],
}));

const autoReply = parseAutoReplyConfig({
  name: "Reply",
  matchStrategy: "all",
  rules: [{
    id: "r1",
    triggerType: "data",
    timerIntervalMs: 1000,
    conditions: [{ pattern: "ping", mode: "contains", caseSensitive: false, negate: false }],
    conditionLogic: "and",
    actions: [{ delayMs: 0, data: "pong", format: "text" }],
    enabled: true,
    cooldownMs: 0,
  }],
});
assert.equal(autoReply.rules.length, 1);
assert.throws(() => parseAutoReplyConfig({
  name: "bad",
  matchStrategy: "all",
  rules: [{
    id: "r1",
    triggerType: "data",
    timerIntervalMs: 0,
    conditions: [],
    conditionLogic: "and",
    actions: [],
    enabled: true,
    cooldownMs: 0,
  }],
}));
assert.throws(() => parseAutoReplyConfig({
  name: "bad timer",
  matchStrategy: "all",
  rules: [{
    id: "r1",
    triggerType: "timer",
    timerIntervalMs: 0,
    conditions: [],
    conditionLogic: "and",
    actions: [],
    enabled: true,
    cooldownMs: 0,
  }],
}));

assert.deepEqual(parseScriptImport({ name: "Demo", code: "print('ok')", ignored: true }), {
  name: "Demo",
  code: "print('ok')",
});
assert.throws(() => parseScriptImport({ name: "Demo" }));
assert.equal(uniqueAssetName("Demo", ["Demo"], "Imported"), "Demo (Imported)");
assert.equal(
  uniqueAssetName("Demo", ["Demo", "Demo (Imported)", "Demo (Imported 2)"], "Imported"),
  "Demo (Imported 3)",
);

// Network target selection may exist while disconnected, but runtime sync starts only once
// the Network plugin owns a connected server-side send target.
const tcpServerParams = { transport: "tcp", role: "server" };
const udpServerParams = { transport: "udp", role: "server" };
assert.equal(isNetworkSendTargetVisible(tcpServerParams), true);
assert.equal(isNetworkSendTargetVisible(udpServerParams), true);
assert.equal(isNetworkSendTargetVisible({ transport: "tcp", role: "client" }), false);
assert.equal(canSyncNetworkSendTarget("disconnected", tcpServerParams), false);
assert.equal(canSyncNetworkSendTarget("connecting", tcpServerParams), false);
assert.equal(canSyncNetworkSendTarget("connected", tcpServerParams), true);
assert.equal(canSyncNetworkSendTarget("connected", udpServerParams), true);
assert.equal(canSyncNetworkSendTarget("connected", { transport: "tcp", role: "client" }), false);

// RTT target-row visibility is runtime authoritative: disconnected/faulted sessions and
// sessions without a healthy Down direction must not reserve target-bar height.
const rttDirection = { buffer_size: 64, usable: true, issue: null };
const rttSnapshot = {
  generation: 1,
  phase: "running",
  backend: null,
  channels: [{
    index: 0,
    name: "Terminal",
    up: rttDirection,
    down: rttDirection,
    metadata_complete: true,
  }],
  automation_source_channel: 0,
  send_channel: 0,
  rx_bytes: 0,
  tx_bytes: 0,
  dropped_history_bytes: 0,
  dropped_history_chunks: 0,
  dropped_automation_bytes: 0,
  dropped_automation_chunks: 0,
  dropped_presentation_bytes: 0,
  dropped_presentation_chunks: 0,
  runtime_pressure_events: 0,
  last_error: null,
};
assert.equal(hasUsableRttDownChannel(rttSnapshot), true);
assert.equal(hasUsableRttDownChannel({ ...rttSnapshot, phase: "faulted" }), false);
assert.equal(hasUsableRttDownChannel({
  ...rttSnapshot,
  channels: [{
    ...rttSnapshot.channels[0],
    down: { buffer_size: 64, usable: false, issue: "invalid descriptor" },
  }],
}), false);

// SendBar splitter geometry is exact in pixel space. Returning to the minimum must
// produce the same canonical body height regardless of container size or plugin send target.
const bodyMinHeight = 156;
const targetBarHeight = 42;
for (const containerHeight of [640, 810, 900, 1200]) {
  assert.equal(clampSendBarBodyHeight(-100, containerHeight, bodyMinHeight), bodyMinHeight);
  assert.equal(
    clampSendBarBodyHeight(-100, containerHeight, bodyMinHeight, targetBarHeight),
    bodyMinHeight,
  );
}
assert.equal(clampSendBarBodyHeight(999, 810, bodyMinHeight), 648);
assert.equal(clampSendBarBodyHeight(999, 810, bodyMinHeight, targetBarHeight), 606);
assert.equal(getSendBarHostHeightCss(null, false), "var(--sendbar-min-height)");
assert.equal(
  getSendBarHostHeightCss(bodyMinHeight, true),
  "calc(156px + var(--sendbar-targetbar-height))",
);
assert.equal(
  getSendBarHostMinHeightCss(true),
  "calc(var(--sendbar-min-height) + var(--sendbar-targetbar-height))",
);

// Architecture contracts: presentation, per-session UI state, shared assets and execution stay separate.
const basicSend = source("src/components/SendBar/BasicSend.tsx");
assert.ok(basicSend.includes("buildSendPayload"));
assert.ok(!basicSend.includes("setInterval("), "repeat sends must provide backpressure");

// Common SendBar only resolves generic plugin contributions. Network target presentation,
// visibility and backend synchronization stay inside the Network plugin.
const sendBar = source("src/components/SendBar/SendBar.tsx");
assert.ok(sendBar.includes("pluginRegistry.get(tab.pluginId)?.sendTarget"));
assert.ok(!sendBar.includes("NetworkSendTarget"), "common SendBar must not import a built-in target implementation");
assert.ok(!sendBar.includes("set_network_send_target"), "common SendBar must not own Network synchronization");
const app = source("src/App.tsx");
assert.ok(app.includes("pluginRegistry.resolveSendTargetVisible"));
assert.ok(app.includes("usePluginRuntimeRevision"));
assert.ok(!app.includes("networkSendTarget"), "app shell must not own Network target rules");
const networkTarget = source("src/plugins/network/NetworkSendTarget.tsx");
assert.ok(networkTarget.includes('invoke("set_network_send_target"'));
assert.ok(networkTarget.includes("canSyncNetworkSendTarget"));
assert.ok(networkTarget.includes("if (!syncReady) return;"));
assert.ok(networkTarget.includes("if (!active) return;"), "stale target-sync failures must not surface after lifecycle changes");
assert.ok(!networkTarget.includes("catch(() =>"), "current target sync failures must not be swallowed");
const networkPlugin = source("src/plugins/network/index.tsx");
assert.ok(networkPlugin.includes("sendTarget: NetworkSendTarget"));
assert.ok(networkPlugin.includes("sendTargetVisible: ({ params }) => isNetworkSendTargetVisible(params)"));

const rttPlugin = source("src/plugins/rtt/index.tsx");
const rttTarget = source("src/plugins/rtt/RttSendTarget.tsx");
const rttRuntime = source("src/plugins/rtt/runtime-store.ts");
assert.ok(rttPlugin.includes("sendTarget: RttSendTarget"));
assert.ok(rttPlugin.includes("hasUsableRttDownChannel"));
assert.ok(rttPlugin.includes("sendData: sendRttData"));
assert.ok(rttPlugin.includes("sendBarEnabled: true"));
assert.ok(rttTarget.includes("selectRttSendChannel"));
assert.ok(rttRuntime.includes('invoke("rtt_set_send_channel"'));
assert.ok(rttRuntime.includes('invoke("rtt_set_automation_source_channel"'));
assert.ok(rttRuntime.includes("selectedChannel"), "RTT viewer Channel must remain plugin-local UI state");
assert.ok(
  rttTarget.includes("runtime.snapshot?.send_channel"),
  "RTT SendBar target must come from the runtime-authoritative snapshot",
);
assert.ok(
  !rttRuntime.includes("selectedSendChannel"),
  "RTT frontend must not reintroduce a second SendBar target authority",
);

const sessionContext = source("src/context/SessionContext.tsx");
const sendTargetCatch = sessionContext.match(
  /const sendToTarget = useCallback\([\s\S]*?catch \(error\) \{([\s\S]*?)\n    \}/,
)?.[1] ?? "";
assert.ok(sendTargetCatch.includes("throw error"), "SendBar write failures must propagate to execution owners");

const context = source("src/components/SendBar/SendBarContext.tsx");
assert.ok(!context.includes("subscribeAsset<string>(\n      ASSET_KEYS.activeScriptId"));
assert.ok(!context.includes("subscribeAsset<string>(\n      ASSET_KEYS.activeAutoReplyConfig"));
assert.ok(context.includes('type: "SET_ACTIVE_COMMAND_CONFIG"'));
assert.ok(context.includes('type: "SET_EXECUTION_MODE"'));
assert.ok(context.includes("executionMode: SendBarMode | null"));
assert.ok(context.includes("hasStoredConfigs ? storedConfigs : [...BUILTIN_CONFIGS]"));
assert.ok(context.includes("hasStoredScripts ? storedScripts : [...BUILTIN_SCRIPTS]"));
assert.ok(context.includes("const hasLocalDraft = previousActive != null && current.code !== previousActive.code"));
assert.ok(context.includes('stateRef.current.executionMode === "auto-reply"'));
assert.ok(context.includes('stateRef.current.executionMode === "script"'));

assert.ok(!sendBar.includes("wrapperHidden"), "inactive mode panels should not stay mounted");
assert.ok(sendBar.includes("const { mode, executionMode } = state"));
assert.ok(sendBar.includes('dispatch({ type: "SET_EXECUTION_MODE", owner, running })'));
assert.ok(!sendBar.includes("useState<"), "execution ownership should live in SendBarContext");
assert.ok(!sendBar.includes("engineSessionId"), "dead optional engine routing API must not return");
assert.ok(sendBar.includes("showTargetBar && SendTarget"));
assert.ok(
  sendBar.includes("<SendTarget sessionId={containerId} disabled={executionMode !== null} />"),
  "plugin target controls must lock with the current SendBar execution snapshot",
);
assert.ok(
  app.includes("showTargetBar={isActive && activeShowTargetBar}"),
  "SendBar host geometry and rendered target row must share one visibility decision",
);
assert.ok(networkTarget.includes("disabled={disabled}"));
assert.ok(rttTarget.includes("disabled={disabled}"));

assert.ok(app.includes("useSendBarLayout"), "App shell must delegate SendBar splitter geometry");
assert.ok(!app.includes("sendBarPct"), "SendBar height must not be stored as a percentage");
assert.ok(!app.includes("SENDBAR_MIN_PCT"), "percentage minimum quantization must not return");
const sendBarLayoutHook = source("src/components/SendBar/useSendBarLayout.ts");
assert.ok(sendBarLayoutHook.includes("clampSendBarBodyHeight"));
assert.ok(sendBarLayoutHook.includes("new ResizeObserver(normalizeHeight)"));

const commandPanel = source("src/components/SendBar/CommandPanel.tsx");
assert.ok(commandPanel.includes("usePointerDragReorder"));
assert.ok(!commandPanel.includes("dragIndexRef"), "command panel must use the shared reorder hook");
assert.ok(!commandPanel.includes("subscribeAsset<string>(ACTIVE_CONFIG_STORE_KEY"));
assert.ok(commandPanel.includes("sendBarState.command"));
assert.ok(commandPanel.includes('sendBarState.executionMode === "command"'));
assert.ok(commandPanel.includes("pendingConfigsRef"), "shared command assets must be deferred while executing");
assert.ok(commandPanel.includes("hasStoredConfigs ? storedConfigs"));
assert.ok(commandPanel.includes("<ConfirmDialog"));

const commandRunner = source("src/components/SendBar/useCommandRunner.ts");
const stopBody = commandRunner.match(/const stop = useCallback\(\(\) => \{([\s\S]*?)\n  \}, \[\]\);/)?.[1] ?? "";
assert.ok(stopBody.includes("stopFlagRef.current = true"));
assert.ok(!stopBody.includes("runningRef.current = false"), "stop must stay locked until in-flight send settles");
assert.ok(!stopBody.includes("setIsRunning(false)"), "stop must stay locked until in-flight send settles");

const autoReplyPanel = source("src/components/SendBar/AutoReplyPanel.tsx");
assert.ok(autoReplyPanel.includes("parseAutoReplyConfig"));
assert.ok(autoReplyPanel.includes("const runtimeLocked = isRunning || isLoading"));
assert.ok(autoReplyPanel.includes("transitionAttemptRef"));
assert.ok(autoReplyPanel.includes("disabled={runtimeLocked}"));
assert.ok(autoReplyPanel.includes("<ConfirmDialog"));

const scriptEditor = source("src/components/SendBar/ScriptEditor.tsx");
assert.ok(scriptEditor.includes("parseScriptImport"));
assert.ok(scriptEditor.includes("const runtimeLocked = isRunning || isTransitioning"));
assert.ok(scriptEditor.includes("transitionAttemptRef"));
assert.ok(scriptEditor.includes("readOnly={runtimeLocked}"));
assert.ok(scriptEditor.includes("<ConfirmDialog"));

const types = source("src/components/SendBar/types.ts");
assert.ok(!types.includes("interface LoopConfig"));
assert.ok(!types.includes("interface ExecutionState"));
assert.ok(!types.includes("localStorage"));

console.log("SendBar payload, layout, plugin-target boundary, lifecycle, execution and UI contracts passed.");
