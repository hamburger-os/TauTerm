import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import path from "node:path";
import {
  getPaneDisplayLabel,
  getSessionSubtitle,
} from "../src/components/Layout/sessionPresentation.ts";
import { pluginRegistry, registerPlugin } from "../src/core/plugin-registry.ts";

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
assert.doesNotMatch(
  presentationHelper,
  /pluginId\s*===\s*["'](?:ssh|iperf|trdp|network|serial|modbus)["']/,
  "Common Session presentation must not contain built-in protocol branches",
);

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

assert.equal(getSessionSubtitle(baseTab), "COM1");
assert.equal(
  getPaneDisplayLabel(baseTab, new Map([[baseTab.id, baseTab]])),
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
registerPlugin({
  manifest: {
    id: pluginTab.pluginId,
    name: "Presentation Contract Test",
    version: "1",
    category: "test",
    description: "",
    icon: "connection",
    content_type: "custom",
    send_bar: false,
    capabilities: [],
    transfer_protocols: [],
  },
  sessionPresentation: {
    subtitle: params => String(params.target ?? ""),
  },
});
assert.equal(
  getSessionSubtitle(pluginTab),
  "192.0.2.10:1234",
  "Plugin-owned dynamic subtitle must override the internal endpoint",
);
pluginRegistry.unregister(pluginTab.pluginId);

for (const plugin of ["ssh", "iperf", "trdp", "network", "serial", "modbus"]) {
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
}

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
assert.equal(getSessionSubtitle(childTab), "Windows PowerShell");
assert.equal(
  getPaneDisplayLabel(childTab, tabsById),
  "Shell @ Windows PowerShell › Shell 1 · Windows PowerShell",
);

console.log("workspace-ui: context-menu, plugin-owned Session presentation and custom-view identity contracts preserved");
