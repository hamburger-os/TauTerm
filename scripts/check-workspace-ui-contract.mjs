import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import path from "node:path";
import {
  getPaneDisplayLabel,
  getSessionSubtitle,
} from "../src/components/Layout/sessionPresentation.ts";
import { setSessionPresentation } from "../src/core/session-presentation-registry.ts";

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

const labels = {
  trdpCapture: "Capture",
  trdpUnconfigured: "Unconfigured",
  trdpDisabled: "Disabled",
};
const baseTab = {
  id: "session-a",
  name: "Serial @ Text",
  connection_type: "serial",
  endpoint: "COM1",
  state: "disconnected",
  pluginId: "serial",
  params: {},
  stats: { txBytes: 0, rxBytes: 0 },
  connectedAt: null,
};

assert.equal(getSessionSubtitle(baseTab, labels), "COM1");
assert.equal(
  getPaneDisplayLabel(baseTab, new Map([[baseTab.id, baseTab]]), labels),
  "Serial @ Text · COM1",
);

const pluginTab = {
  ...baseTab,
  id: "plugin-a",
  name: "Protocol @ Role",
  pluginId: "presentation-contract-test",
  connection_type: "presentation-contract-test",
  endpoint: "internal-endpoint",
  params: { target: "192.0.2.10:1234" },
};
setSessionPresentation(pluginTab.pluginId, {
  subtitle: params => String(params.target ?? ""),
});
assert.equal(
  getSessionSubtitle(pluginTab, labels),
  "192.0.2.10:1234",
  "Plugin-owned dynamic subtitle must override the internal endpoint",
);
setSessionPresentation(pluginTab.pluginId);

const sshTab = {
  ...baseTab,
  id: "ssh-a",
  name: "SSH @ root",
  pluginId: "ssh",
  connection_type: "ssh",
  endpoint: "2001:db8::10",
  params: { host: "2001:db8::10", port: 2222 },
};
assert.equal(getSessionSubtitle(sshTab, labels), "[2001:db8::10]:2222");

const trdpTab = {
  ...baseTab,
  id: "trdp-a",
  name: "TRDP Monitor",
  pluginId: "trdp",
  connection_type: "trdp",
  endpoint: "",
  params: {
    mode: "monitor",
    capture_interface: "eth0",
    capture_interface_b_enabled: true,
    capture_interface_b: "",
  },
};
assert.equal(
  getSessionSubtitle(trdpTab, labels),
  "A: eth0 · B: Unconfigured",
);

const networkTab = {
  ...baseTab,
  id: "network-a",
  name: "Network Debug @ TCP Client",
  pluginId: "network",
  connection_type: "network",
  endpoint: "tcp://127.0.0.1:8080",
  params: { role: "client", transport: "tcp" },
};
const networkState = {
  networkLocalAddrs: {},
  networkPeers: {
    "network-a": [{
      peerId: "peer-a",
      name: "Peer 1",
      addr: "127.0.0.1:8080",
      localAddr: "127.0.0.1:51000",
      state: "connected",
      txBytes: 0,
      rxBytes: 0,
    }],
  },
};
assert.equal(
  getSessionSubtitle(networkTab, labels, networkState),
  "tcp://127.0.0.1:8080 · 127.0.0.1:51000",
);

const parentTab = {
  ...baseTab,
  id: "shell-root",
  name: "Shell @ Windows PowerShell",
  pluginId: "local-shell",
  connection_type: "local-shell",
  endpoint: "Windows PowerShell",
};
const childTab = {
  ...parentTab,
  id: "shell-child",
  name: "Shell 1",
  endpoint: "Windows PowerShell",
  parentId: parentTab.id,
};
const tabsById = new Map([
  [parentTab.id, parentTab],
  [childTab.id, childTab],
]);
assert.equal(getSessionSubtitle(childTab, labels), "Windows PowerShell");
assert.equal(
  getPaneDisplayLabel(childTab, tabsById, labels),
  "Shell @ Windows PowerShell › Shell 1 · Windows PowerShell",
);

console.log("workspace-ui: context-menu, Session presentation and custom-view identity contracts preserved");
