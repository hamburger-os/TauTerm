import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import path from "node:path";
import { resolveSessionSubtitle } from "../src/core/plugin-contracts.ts";

const ROOT = process.cwd();
const splitView = await readFile(
  path.join(ROOT, "src", "components", "Layout", "SplitView.tsx"),
  "utf8",
);
const sidebar = await readFile(
  path.join(ROOT, "src", "components", "Layout", "SessionSidebar.tsx"),
  "utf8",
);
const customRenderer = await readFile(
  path.join(ROOT, "src", "renderers", "CustomRenderer.tsx"),
  "utf8",
);
const presentationHelper = await readFile(
  path.join(ROOT, "src", "components", "Layout", "sessionPresentation.ts"),
  "utf8",
);
const connectDialog = await readFile(
  path.join(ROOT, "src", "components", "Layout", "ConnectDialog.tsx"),
  "utf8",
);
const sessionContext = await readFile(
  path.join(ROOT, "src", "context", "SessionContext.tsx"),
  "utf8",
);
const disconnectedSessionMenu = await readFile(
  path.join(ROOT, "src", "components", "Layout", "DisconnectedSessionContextMenu.tsx"),
  "utf8",
);
const tftpRegistration = await readFile(
  path.join(ROOT, "src", "plugins", "tftp", "index.ts"),
  "utf8",
);
const tftpConnectForm = await readFile(
  path.join(ROOT, "src", "plugins", "tftp", "TftpConnectForm.tsx"),
  "utf8",
);
const iperfRegistration = await readFile(
  path.join(ROOT, "src", "plugins", "iperf", "index.ts"),
  "utf8",
);
const networkRegistration = await readFile(
  path.join(ROOT, "src", "plugins", "network", "index.tsx"),
  "utf8",
);
const tftpCommands = await readFile(
  path.join(ROOT, "src-tauri", "src", "plugins", "tftp", "commands.rs"),
  "utf8",
);
const iperfCommands = await readFile(
  path.join(ROOT, "src-tauri", "src", "plugins", "iperf", "commands.rs"),
  "utf8",
);
const trdpManifest = JSON.parse(await readFile(
  path.join(ROOT, "src", "plugin-manifests", "trdp.json"),
  "utf8",
));

function sliceBetween(source, startMarker, endMarker) {
  const start = source.indexOf(startMarker);
  assert.notEqual(start, -1, `Missing source marker: ${startMarker}`);
  const end = source.indexOf(endMarker, start + startMarker.length);
  assert.notEqual(end, -1, `Missing source marker: ${endMarker}`);
  return source.slice(start, end);
}

assert.doesNotMatch(
  splitView,
  /onMouseDown=\{closeDisconnectedSessionMenu\}/,
  "Workspace root must not close a Portal context menu during mousedown before its menu-item click can run",
);
assert.match(
  splitView,
  /getPaneDisplayLabel/,
  "Pane headers must use the shared Session presentation contract",
);
assert.match(
  splitView,
  /plugin\?\.workspace\?\.availability/,
  "SplitView must delegate disconnected workspace visibility to the plugin contribution",
);
assert.match(
  splitView,
  /workspaceAvailability\s*!==\s*["']always["']/,
  "Offline-capable workspaces must remain renderable without a connected runtime",
);
assert.match(
  splitView,
  /retainedNonTerminalSessionIds/,
  "Non-terminal workspace views must be retained by Session identity while switching Pane assignments",
);
assert.match(
  splitView,
  /nonTerminalSessionPoolIds\.map\(sessionId\s*=>/,
  "Retained non-terminal Sessions must render from a stable Session-owned instance pool",
);
assert.match(
  splitView,
  /plugin\?\.workspace\?\.availability\s*===\s*["']always["']\s*\|\|\s*tab\.state\s*!==\s*["']disconnected["']/,
  "Connected-only workspace views must release on disconnect while offline-capable workspaces remain alive",
);
assert.match(
  splitView,
  /placement[\s\S]*?\{\s*display:\s*["']none["']\s*\}/,
  "Background Session views must be hidden instead of unmounted when they are not assigned to a Pane",
);
assert.doesNotMatch(
  splitView,
  /tab\s*&&\s*!isTerminal\s*&&\s*!showDisconnectedPlaceholder\s*&&\s*renderNonTerminalContent\(tab\)/,
  "Pane surfaces must not directly own non-terminal Session component lifetime",
);
assert.match(
  sidebar,
  /getSessionSubtitle/,
  "Session cards must use the shared Session presentation contract",
);
assert.doesNotMatch(
  sidebar,
  /id:\s*["']connect["'][^}\n]*icon:\s*["']connection["']/,
  "Every connect action must use the play icon regardless of Session state",
);
assert.match(
  customRenderer,
  /key=\{`\$\{tab\.pluginId\}:\$\{tab\.id\}`\}/,
  "Custom renderer views must be keyed by plugin and Session identity so Pane reuse cannot leak component-local state across Sessions",
);
assert.match(
  presentationHelper,
  /pluginRegistry\.get\(tab\.pluginId\)\?\.sessionPresentation/,
  "Common Session presentation must resolve descriptors from the single PluginRegistry",
);
assert.match(
  presentationHelper,
  /resolveSessionSubtitle/,
  "Common Session presentation must use the dependency-free resolver contract",
);
assert.doesNotMatch(
  presentationHelper,
  /pluginId\s*===\s*["'](?:ssh|iperf|trdp|network|serial|modbus)["']/,
  "Common Session presentation must not contain built-in protocol branches",
);

for (const [label, source] of [
  ["SessionSidebar", sidebar],
  ["SplitView", splitView],
  ["SessionContext", sessionContext],
  ["DisconnectedSessionContextMenu", disconnectedSessionMenu],
  ["sessionPresentation", presentationHelper],
]) {
  assert.doesNotMatch(source, /\bshell_kind\b/, `${label} must not interpret Local Shell private configuration fields`);
}
assert.doesNotMatch(
  connectDialog,
  /\b(?:isSerial|isSsh|isTftp|isTelnet|isIperf|isNetwork|isTrdp|isLocalShell)\b|["'](?:serial|ssh|tftp|telnet|iperf|network|trdp|modbus|local-shell)["']/,
  "ConnectDialog must not contain built-in plugin branches or IDs",
);
for (const contribution of ["defaultConnectionParams", "prepareConnectionParams", "resolveEndpoint"]) {
  assert.ok(connectDialog.includes(contribution), `ConnectDialog must delegate ${contribution} to PluginRegistration`);
}
assert.match(
  connectDialog,
  /label:\s*plugin\.manifest\.name/,
  "New Session cards and configuration headers must share the canonical plugin name",
);
assert.doesNotMatch(
  connectDialog,
  /description:\s*plugin\.manifest\.description/,
  "Plugin description must not be used as the Session type identity",
);
assert.equal(
  trdpManifest.name,
  "TRDP 调试助手",
  "TRDP canonical Session type name must remain aligned with the new-session card and configuration header",
);
assert.match(sidebar, /canCreateElevatedSession/, "SessionSidebar must delegate elevated-session policy to the plugin contribution");
assert.match(disconnectedSessionMenu, /canCreateElevatedSession/, "Disconnected Pane menu must delegate elevated-session policy to the plugin contribution");
assert.doesNotMatch(
  sessionContext,
  /exposure_confirmed|write_enabled|overwrite|listen_ip/,
  "SessionContext must not interpret TFTP reconnect-safety fields",
);
assert.match(
  tftpRegistration,
  /reconnectGuard\s*:/,
  "TFTP must own its reconnect safety policy through PluginRegistration",
);
assert.match(
  tftpRegistration,
  /workspace:\s*\{\s*availability:\s*["']always["']\s*\}/,
  "TFTP client workspace must remain available while its server runtime is disconnected",
);
assert.match(
  iperfRegistration,
  /workspace:\s*\{\s*availability:\s*["']always["']\s*\}/,
  "iperf client workspace must remain available while its server runtime is disconnected",
);
assert.match(
  tftpConnectForm,
  /exposure_confirmed/,
  "TFTP exposure guard must have an explicit configuration-form acknowledgement path",
);
assert.match(
  tftpConnectForm,
  /hasTftpExposureRisk/,
  "TFTP form and reconnect guard must share one exposure-risk predicate",
);
assert.match(
  networkRegistration,
  /prepareConnectionParams\s*:/,
  "Network must own its connection-parameter projection through PluginRegistration",
);
assert.match(
  networkRegistration,
  /local_port:\s*0/,
  "Network client sessions must keep ephemeral local-port binding semantics in the plugin contribution",
);

const tftpGet = sliceBetween(tftpCommands, "pub async fn tftp_client_get", "pub async fn tftp_client_put");
const tftpPut = sliceBetween(tftpCommands, "pub async fn tftp_client_put", "fn sync_tftp_server_params");
for (const [label, source] of [["GET", tftpGet], ["PUT", tftpPut]]) {
  assert.match(source, /Uuid::new_v4\(\)/, `TFTP ${label} must create a transient client transfer identity`);
  assert.doesNotMatch(
    source,
    /runtime\(&session_id\)[\s\S]*?ok_or_else/,
    `TFTP ${label} must not require a connected server runtime`,
  );
}
assert.match(
  tftpCommands,
  /pub async fn tftp_get_status[\s\S]*?server_running:\s*false[\s\S]*?listen_addr:\s*None/,
  "TFTP disconnected workbench status must degrade to a valid stopped/default state",
);

const iperfClientRun = sliceBetween(iperfCommands, "pub async fn iperf_client_run", "fn sanitize_iperf_params");
assert.match(
  iperfClientRun,
  /None\s*=>[\s\S]*?IPERF_CLIENT_REGISTRY/,
  "iperf client runs must provide transient state when no connected runtime exists",
);
assert.doesNotMatch(
  iperfClientRun,
  /runtime\(&session_id\)[\s\S]*?ok_or_else/,
  "iperf client runs must not require a connected server runtime",
);

const baseSession = {
  endpoint: "COM1",
  params: {},
  parentId: null,
};
assert.equal(resolveSessionSubtitle(undefined, baseSession), "COM1");

const descriptor = {
  subtitle: params => String(params.target ?? ""),
};
assert.equal(
  resolveSessionSubtitle(descriptor, {
    endpoint: "internal-endpoint",
    params: { target: "192.0.2.10:1234" },
    parentId: null,
  }),
  "192.0.2.10:1234",
  "Plugin-owned dynamic subtitle must override the internal endpoint",
);
assert.equal(
  resolveSessionSubtitle(descriptor, {
    endpoint: "Windows PowerShell",
    params: { target: "must-not-override-child" },
    parentId: "shell-root",
  }),
  "Windows PowerShell",
  "Child terminal identity must continue to use its runtime endpoint",
);

for (const plugin of ["ssh", "tftp", "telnet", "iperf", "local-shell", "trdp", "network", "serial", "modbus"]) {
  const extension = plugin === "modbus" || plugin === "trdp" || plugin === "network" ? "tsx" : "ts";
  const source = await readFile(
    path.join(ROOT, "src", "plugins", plugin, `index.${extension}`),
    "utf8",
  );
  assert.match(
    source,
    /sessionPresentation\s*:/,
    `${plugin} must own its Session presentation contribution`,
  );
  assert.match(
    source,
    /connectForm\s*:/,
    `${plugin} must own its connection form contribution`,
  );
}

console.log("workspace-ui: plugin-owned Session identity, retained Session workspaces, offline workbenches, context menus, runtime independence and reconnect safety contracts preserved");
