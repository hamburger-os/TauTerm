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
const networkRegistration = await readFile(
  path.join(ROOT, "src", "plugins", "network", "index.tsx"),
  "utf8",
);

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
  sidebar,
  /getSessionSubtitle/,
  "Session cards must use the shared Session presentation contract",
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
  networkRegistration,
  /prepareConnectionParams\s*:/,
  "Network must own its connection-parameter projection through PluginRegistration",
);
assert.match(
  networkRegistration,
  /local_port:\s*0/,
  "Network client sessions must keep ephemeral local-port binding semantics in the plugin contribution",
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

console.log("workspace-ui: context-menu, plugin-owned Session presentation and custom-view identity contracts preserved");
