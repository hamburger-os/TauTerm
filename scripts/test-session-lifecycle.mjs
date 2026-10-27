import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { runInNewContext } from "node:vm";
import ts from "typescript";
import { AsyncRequestScope, RuntimeEventCursor, compareRuntimeIds } from "../src/core/session-lifecycle.ts";
import { initialSessionState, sessionReducer } from "../src/core/session-state.ts";
import { initialTransferState, transferReducer, latestActiveTaskForSession } from "../src/core/transfer-state.ts";

const tab = (runtimeId, extra = {}) => ({
  id: "saved", runtimeId, name: "Session", connection_type: "dummy", pluginId: "dummy",
  endpoint: "local", state: "connected", stats: { txBytes: 0, rxBytes: 0 }, connectedAt: 1, ...extra,
});
let sessions = sessionReducer(initialSessionState, { type: "SESSION_CONNECTED", tab: tab("1") });
sessions = sessionReducer(sessions, { type: "SESSION_DISCONNECTED", id: "saved", runtimeId: "1" });
assert.equal(sessions.tabs[0].state, "disconnected");
const disconnected = sessions;
sessions = sessionReducer(sessions, { type: "SESSION_CONNECTED", tab: tab("1") });
assert.strictEqual(sessions, disconnected, "a delayed connected event must not resurrect a closed runtime");
sessions = sessionReducer(sessions, { type: "SESSION_CONNECTED", tab: tab("2") });
const reconnected = sessions;
assert.strictEqual(sessionReducer(sessions, { type: "SESSION_CONNECTED", tab: tab("2") }), sessions,
  "duplicate connected events must not reset statistics or change selection");
for (const action of [
  { type: "SESSION_DISCONNECTED", id: "saved", runtimeId: "1" },
  { type: "SESSION_CONNECTED", tab: tab("1") },
  { type: "UPDATE_TAB_STATS", id: "saved", runtimeId: "1", stats: { txBytes: 900, rxBytes: 900 } },
  { type: "SESSION_CONNECTED", tab: tab("1", { id: "old-child", parentId: "saved" }) },
]) {
  sessions = sessionReducer(sessions, action);
  assert.equal(sessions.tabs.length, 1);
  assert.equal(sessions.tabs[0].runtimeId, "2");
  assert.equal(sessions.tabs[0].state, "connected");
  assert.equal(sessions.tabs[0].stats.rxBytes, 0);
}
assert.equal(compareRuntimeIds("9007199254740993", "9007199254740992"), 1, "u64 identities retain ordering beyond JS Number precision");
sessions = sessionReducer(sessions, { type: "SESSION_CONNECTED", tab: tab("2", { id: "child", parentId: "saved" }) });
assert.equal(sessions.tabs.length, 2, "children belong to their root runtime");
const childClosed = sessionReducer(sessions, { type: "REMOVE_CHILD", id: "child", parentId: "saved" });
assert.equal(childClosed.tabs[0].state, "connected", "child removal cannot infer the parent connection state");
const rootClosed = sessionReducer(sessions, { type: "SESSION_DISCONNECTED", id: "saved", runtimeId: "2" });
assert.equal(rootClosed.activeTabId, "saved", "removing the active child restores the root context");
const unrelatedActive = { ...sessions, activeTabId: "other" };
assert.equal(sessionReducer(unrelatedActive, { type: "SESSION_DISCONNECTED", id: "saved", runtimeId: "2" }).activeTabId,
  "other", "disconnecting a background root does not steal selection");
const processExit = { kind: "process_exited", reason: "exit 23", exit_code: 23, retain_terminal: true };
const retained = sessionReducer(sessions, { type: "SET_TAB_DISCONNECTED", id: "child", info: processExit });
const rootEof = { kind: "remote_eof", reason: "all terminals closed", retain_terminal: true };
const retainedRootClosed = sessionReducer(retained, { type: "SESSION_DISCONNECTED", id: "saved", runtimeId: "2", info: rootEof });
assert.strictEqual(retainedRootClosed.tabs.find(item => item.id === "child").disconnectInfo, processExit,
  "container EOF preserves the child's specific exit reason and code");
assert.equal(retainedRootClosed.activeTabId, "child", "retaining the terminal retains its active context");
sessions = sessionReducer(sessions, { type: "SESSION_CONNECTED", tab: tab("3") });
assert.equal(sessions.tabs.length, 1, "a new root does not inherit children from an old runtime");

const started = (id, runtime = "2") => ({ session_id: "saved", runtime_id: runtime, transfer_id: id, protocol: "sftp", direction: "send" });
const progress = (id, runtime = "2") => ({ ...started(id, runtime), kind: "progress", file_name: "file.bin", bytes_done: 4, bytes_total: 8,
  bytes_per_second: 4, file_index: 0, total_files: 1, aggregate_bytes: 4, aggregate_total: 8, file_success: null, file_error: null });
const finished = (id, runtime = "2") => ({ ...started(id, runtime), success: true, cancelled: false, error: null, results: [] });
let transfers = transferReducer(initialTransferState, { type: "TASK_STARTED", payload: started("job") });
transfers = transferReducer(transfers, { type: "TASK_FINISHED", payload: finished("job") });
const terminal = transfers;
assert.strictEqual(transferReducer(transfers, { type: "TASK_PROGRESS", payload: progress("job") }), terminal, "late progress cannot restart a terminal task");
transfers = transferReducer(transfers, { type: "TASK_ACKNOWLEDGED", payload: started("job"), initialFiles: [{ fileName: "late.bin", status: "pending", bytesTransferred: 0, totalBytes: 8 }] });
assert.equal(transfers.tasksById.job.phase, "completed");
assert.equal(transfers.tasksById.job.files[0].status, "skipped", "an ACK arriving after finished must not leave pending files");
transfers = transferReducer(transfers, { type: "TASK_STARTED", payload: started("new-job", "3") });
transfers = transferReducer(transfers, { type: "TASK_DISCARD_SESSION", sessionId: "saved", runtimeId: "2" });
assert.equal(latestActiveTaskForSession(transfers, "saved")?.transferId, "new-job", "old disconnect preserves new-runtime activity");
transfers = transferReducer(transfers, { type: "TASK_FINISHED", payload: finished("new-job", "2") });
assert.equal(transfers.tasksById["new-job"].phase, "preparing", "wrong-runtime terminal event is ignored");
let sameRuntime = transferReducer(initialTransferState, { type: "TASK_STARTED", payload: started("A") });
sameRuntime = transferReducer(sameRuntime, { type: "TASK_FINISHED", payload: finished("A") });
sameRuntime = transferReducer(sameRuntime, { type: "TASK_STARTED", payload: started("B") });
const onlyB = sameRuntime;
sameRuntime = transferReducer(sameRuntime, { type: "TASK_ACKNOWLEDGED", payload: started("A") });
sameRuntime = transferReducer(sameRuntime, { type: "TASK_PROGRESS", payload: progress("A") });
assert.strictEqual(sameRuntime, onlyB, "a retired task cannot be recreated by its delayed ACK or progress");
assert.equal(reconnected.tabs[0].state, "connected", "transfer state cannot change connection health");

const cursor = new RuntimeEventCursor();
assert.equal(cursor.accept("1", true), true);
assert.equal(cursor.accept("1", false), true);
assert.equal(cursor.accept("1", true), false);
assert.equal(cursor.accept("2", true), true);
assert.equal(cursor.accept("1", false), false);
assert.equal(cursor.matches("2"), true);

// Resolve real promises out of order against the same production scope used by plugin queries.
const scope = new AsyncRequestScope();
const deferred = () => { let resolve; const promise = new Promise(done => { resolve = done; }); return { promise, resolve }; };
const results = [];
const first = deferred(); const firstCurrent = scope.begin();
const old = first.promise.then(value => { if (firstCurrent()) results.push(value); });
scope.invalidate(); // disconnect, including keepAlive stores
const second = deferred(); const secondCurrent = scope.begin();
const next = second.promise.then(value => { if (secondCurrent()) results.push(value); });
second.resolve("current"); first.resolve("retired");
await Promise.all([old, next]);
assert.deepEqual(results, ["current"]);
const earlier = scope.begin(); const latest = scope.begin();
assert.equal(earlier(), false, "only the latest status request can commit");
assert.equal(latest(), true);
scope.invalidate(); // permanent release
assert.equal(latest(), false);
assert.equal(new AsyncRequestScope().begin()(), true, "recreating an ID uses a distinct scope, without ABA");

// Execute the real plugin store; only Tauri IPC/event delivery is replaced.
function loadModule(path, requireMock) {
  const code = ts.transpileModule(readFileSync(new URL(path, import.meta.url), "utf8"), {
    compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022, jsx: ts.JsxEmit.ReactJSX },
  }).outputText;
  const exports = {};
  runInNewContext(code, { exports, require: requireMock, console, setTimeout, clearTimeout, Uint8Array, TextDecoder, TextEncoder, atob, btoa });
  return exports;
}
const noImports = name => { throw new Error(`Unexpected import: ${name}`); };
const lifecycle = loadModule("../src/core/session-lifecycle.ts", noImports);

// Run the production event ingress, with only React scheduling and Tauri boundaries replaced.
const sessionHandlers = new Map();
const sessionEffects = [];
const sessionContext = loadModule("../src/context/SessionContext.tsx", name => {
  if (name === "react") return {
    createContext: () => ({ Provider: "provider" }), useContext: () => null,
    useCallback: callback => callback, useRef: value => ({ current: value }),
    useEffect: effect => sessionEffects.push(effect),
    useState: initial => {
      let value = typeof initial === "function" ? initial() : initial;
      return [value, next => { value = typeof next === "function" ? next(value) : next; }];
    },
  };
  if (name === "react/jsx-runtime") return { jsx: (_type, props) => ({ props }) };
  if (name.includes("session-lifecycle")) return lifecycle;
  if (name.includes("session-state")) return { initialSessionState, sessionReducer };
  if (name.includes("plugin-registry")) return { pluginRegistry: { get: () => undefined, resolveSessionOptions: () => ({}) } };
  if (name.includes("usePluginSessionStore")) return { releaseSessionStore: () => {} };
  if (name.includes("i18n")) return { default: { t: key => key } };
  if (name === "@tauri-apps/api/core") return { invoke: () => new Promise(() => {}) };
  if (name === "@tauri-apps/api/event") return { listen: async (event, handler) => {
    sessionHandlers.set(event, handler); return () => sessionHandlers.delete(event);
  } };
  return noImports(name);
});
const contextValue = sessionContext.SessionProvider({ children: null }).props.value;
const sessionCleanups = sessionEffects.map(effect => effect());
await flush();
const sessionEvent = (name, payload) => {
  assert.ok(sessionHandlers.has(name), `production listener missing: ${name}`);
  sessionHandlers.get(name)({ payload });
};
const connectedPayload = (id, runtimeId, parentId = null) => ({
  session_id: id, runtime_id: runtimeId, parent_id: parentId,
  plugin_id: "dummy", connection_type: "dummy", endpoint: "local", params: {},
});
const delivered = [];
contextValue.onSessionData((id, bytes) => delivered.push([id, new TextDecoder().decode(bytes)]));
sessionEvent("session-connected", connectedPayload("root", "1"));
sessionEvent("session-connected", connectedPayload("terminal", "1", "root"));
sessionEvent("channel-closed", { channel_id: "terminal", parent_id: "root", runtime_id: "1", disconnect_info: processExit });
sessionEvent("session-disconnected", { session_id: "root", runtime_id: "1", disconnect_info: rootEof });
sessionEvent("session-data", { session_id: "terminal", runtime_id: "1", data_b64: btoa("final diagnostic") });
assert.deepEqual(delivered, [["terminal", "final diagnostic"]], "EOF cannot discard a current-runtime queued tail batch");
sessionEvent("session-connected", connectedPayload("root", "2"));
sessionEvent("session-data", { session_id: "terminal", runtime_id: "1", data_b64: btoa("retired child") });
sessionEvent("session-data", { session_id: "root", runtime_id: "1", data_b64: btoa("retired root") });
assert.equal(delivered.length, 1, "reconnect still rejects all retired-runtime batches");
for (const cleanup of sessionCleanups) cleanup?.();

const model = loadModule("../src/plugins/rtt/model.ts", noImports);
const handlers = new Map();
const requests = new Map();
const rtt = loadModule("../src/plugins/rtt/runtime-store.ts", name => {
  if (name.includes("session-lifecycle")) return lifecycle;
  if (name === "./model") return model;
  if (name === "@tauri-apps/api/core") return { invoke: command => new Promise((resolve, reject) => {
    const queue = requests.get(command) ?? [];
    queue.push({ resolve, reject }); requests.set(command, queue);
  }) };
  if (name === "@tauri-apps/api/event") return { listen: async (event, handler) => {
    handlers.set(event, handler); return () => handlers.delete(event);
  } };
  return noImports(name);
});
async function flush() { for (let i = 0; i < 20; i++) await Promise.resolve(); }
const takeRequest = command => {
  const request = requests.get(command)?.shift();
  assert.ok(request, `${command} must have reached the mocked IPC boundary`);
  return request;
};
const snapshot = (phase, generation = 1) => ({ generation, phase, channels: [], last_error: null });
const query = rtt.refreshRttRuntime("s");
await flush();
handlers.get("rtt-event")({ payload: { session_id: "s", kind: "snapshot", snapshot: snapshot("faulted") } });
takeRequest("rtt_snapshot").resolve(snapshot("running"));
await query;
assert.equal(rtt.rttRuntimeStore.getSnapshot("s").snapshot.phase, "faulted", "a same-generation query cannot overwrite a later terminal event");

const oldHistory = rtt.ensureRttHistory("s", 0);
const oldSystemView = rtt.ensureSystemViewHistory("s", 0);
const oldClear = rtt.clearSystemView("s", 0);
await flush();
rtt.rttRuntimeStore.release("s");
takeRequest("rtt_history").reject(new Error("retired history"));
takeRequest("rtt_systemview_snapshot").reject(new Error("retired SystemView"));
takeRequest("rtt_systemview_history").resolve({ generation: 1, events: [] });
takeRequest("rtt_systemview_clear").resolve();
await Promise.all([oldHistory, oldSystemView, oldClear]);
assert.equal(rtt.rttRuntimeStore.getSnapshot("s").snapshot, null);
assert.equal(rtt.rttRuntimeStore.getSnapshot("s").error, null, "released requests cannot recreate the plugin store");
handlers.get("rtt-event")({ payload: { session_id: "s", kind: "snapshot", snapshot: snapshot("running") } });
assert.equal(rtt.rttRuntimeStore.getSnapshot("s").snapshot, null, "late events cannot recreate a released registration");

const earlierQuery = rtt.refreshRttRuntime("s");
await flush();
const earlierRequest = takeRequest("rtt_snapshot");
const latestQuery = rtt.refreshRttRuntime("s");
await flush();
takeRequest("rtt_snapshot").resolve(snapshot("running", 3));
await latestQuery;
earlierRequest.resolve(snapshot("running", 2));
await earlierQuery;
assert.equal(rtt.rttRuntimeStore.getSnapshot("s").snapshot.generation, 3, "new registration only accepts its latest request");
rtt.rttRuntimeStore.release("s");

const streamDisplay = loadModule("../src/utils/streamDisplay.ts", noImports);
const presentation = loadModule("../src/plugins/network/presentation.ts", name => name.includes("streamDisplay") ? streamDisplay : noImports(name));
const networkHandlers = new Map();
const networkRequests = [];
const network = loadModule("../src/plugins/network/runtime-store.ts", name => {
  if (name.includes("session-lifecycle")) return lifecycle;
  if (name.includes("streamDisplay")) return streamDisplay;
  if (name === "./presentation") return presentation;
  if (name === "@tauri-apps/api/core") return { invoke: (command, args) => new Promise(resolve => networkRequests.push({ command, args, resolve })) };
  if (name === "@tauri-apps/api/event") return { listen: async (event, callback) => {
    networkHandlers.set(event, callback); return () => networkHandlers.delete(event);
  } };
  return noImports(name);
});
const networkEvent = (name, payload) => networkHandlers.get(name)({ payload });
const connectNetwork = runtimeId => {
  networkEvent("netdbg-peer-joined", { session_id: "n", runtime_id: runtimeId, peer_id: `peer-${runtimeId}`, peer_name: "Peer", peer_addr: "127.0.0.1:1" });
  networkEvent("session-connected", { session_id: "n", runtime_id: runtimeId, plugin_id: "network", params: { transport: "tcp", encoding: "utf-8" } });
};
await network.prepareNetworkRuntime("n");
connectNetwork("1");
networkEvent("session-data", { session_id: "peer-1", runtime_id: "1", data_b64: btoa("FIRST-PACKET\n") });
assert.equal(network.getNetworkRuntime("n").frames["peer-1"][0].text, "FIRST-PACKET", "early RX is retained before any React view subscribes");
await flush();
const staleStatsQuery = networkRequests.shift();
networkEvent("session-stats", { tab_id: "peer-1", runtime_id: "1", tx_bytes: 0, rx_bytes: 100 });
staleStatsQuery.resolve({ runtime_id: "1", peers: [{ peer_id: "peer-1", name: "Peer", addr: "127.0.0.1:1", state: "connected", tx_bytes: 0, rx_bytes: 0 }] });
await flush();
assert.equal(network.getNetworkRuntime("n").peers[0].rxBytes, 100, "a stale peer query cannot roll back newer live statistics");
networkEvent("netdbg-peer-left", { session_id: "n", runtime_id: "1", peer_id: "peer-1", rx_bytes: 100 });
networkEvent("session-data", { session_id: "peer-1", runtime_id: "1", data_b64: btoa("FINAL-BATCH\n") });
assert.equal(network.getNetworkRuntime("n").frames["peer-1"].at(-1).text, "FINAL-BATCH", "a closed server peer retains routing for its final batched data");
const retiredRefresh = network.refreshNetworkPeers("n");
await flush();
const retiredPeerQuery = networkRequests.shift();
assert.equal(retiredPeerQuery.command, "list_network_peers");
networkEvent("session-data", { session_id: "peer-1", runtime_id: "1", data_b64: btoa("pending frame") });
network.networkRuntimeStore.release("n");
retiredPeerQuery.resolve({ runtime_id: "1", peers: [] });
await retiredRefresh;
networkEvent("netdbg-peer-joined", { session_id: "n", runtime_id: "1", peer_id: "retired-peer" });
await new Promise(resolve => setTimeout(resolve, 75));
assert.equal(Object.keys(network.getNetworkRuntime("n").frames).length, 0, "release cancels pending framers and rejects late peer queries/events");
await network.prepareNetworkRuntime("n");
connectNetwork("2");
networkEvent("session-data", { session_id: "peer-1", runtime_id: "1", data_b64: btoa("retired\n") });
assert.equal(Object.keys(network.getNetworkRuntime("n").frames).length, 0);
await network.sendNetworkData({ sessionId: "n", runtimeId: "2", params: { transport: "tcp" }, data: "original text", sendDefault: async () => new TextEncoder().encode("actual-wire-bytes") });
assert.equal(network.getNetworkRuntime("n").frames["peer-2"][0].text, "actual-wire-bytes", "TX projection uses confirmed wire bytes");
networkEvent("session-connected", { session_id: "n", runtime_id: "3", plugin_id: "network", params: { transport: "udp" } });
networkEvent("session-data", { session_id: "n", runtime_id: "3", source_addr: "127.0.0.1:2", data_b64: btoa("udp") });
assert.equal(network.getNetworkRuntime("n").packets[0].text, "udp");
assert.equal(network.getNetworkRuntime("n").rxPackets, 1);
network.networkRuntimeStore.release("n");
for (const request of networkRequests) request.resolve(null);
await flush();

console.log("Session lifecycle: runtime identity, child ownership, transfer terminal state, query races and real RTT/Network store regressions passed.");
