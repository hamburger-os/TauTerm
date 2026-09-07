import assert from "node:assert/strict";
import { readFile, readdir } from "node:fs/promises";
import path from "node:path";

const ROOT = process.cwd();
const manifestDir = path.join(ROOT, "src", "plugin-manifests");
const manifestFiles = (await readdir(manifestDir))
  .filter(name => name.endsWith(".json"))
  .sort();

assert.deepEqual(
  manifestFiles,
  [
    "iperf.json",
    "local-shell.json",
    "network.json",
    "serial.json",
    "ssh.json",
    "telnet.json",
    "tftp.json",
    "trdp.json",
  ],
  "canonical built-in plugin manifest set changed; update the product-integrity contract intentionally",
);

const requiredStringFields = [
  "id",
  "name",
  "version",
  "category",
  "description",
  "icon",
  "content_type",
];
const ids = new Set();

for (const file of manifestFiles) {
  const manifest = JSON.parse(await readFile(path.join(manifestDir, file), "utf8"));
  for (const field of requiredStringFields) {
    assert.equal(typeof manifest[field], "string", `${file}: ${field} must be a string`);
    assert.ok(manifest[field].length > 0, `${file}: ${field} must not be empty`);
  }
  assert.equal(typeof manifest.send_bar, "boolean", `${file}: send_bar must be boolean`);
  assert.ok(Array.isArray(manifest.capabilities), `${file}: capabilities must be an array`);
  assert.ok(Array.isArray(manifest.transfer_protocols), `${file}: transfer_protocols must be an array`);
  assert.equal(path.basename(file, ".json"), manifest.id, `${file}: filename and id must match`);
  assert.ok(!ids.has(manifest.id), `${file}: duplicate plugin id ${manifest.id}`);
  ids.add(manifest.id);

  const frontendExt = manifest.id === "network" || manifest.id === "trdp" ? "tsx" : "ts";
  const frontend = await readFile(
    path.join(ROOT, "src", "plugins", manifest.id, `index.${frontendExt}`),
    "utf8",
  );
  assert.match(
    frontend,
    new RegExp(`plugin-manifests/${manifest.id}\\.json`),
    `${manifest.id}: frontend must consume canonical manifest`,
  );
  assert.doesNotMatch(
    frontend,
    /manifest:\s*\{/,
    `${manifest.id}: inline manifest duplicates are forbidden`,
  );
}

const lib = await readFile(path.join(ROOT, "src-tauri", "src", "lib.rs"), "utf8");
for (const file of manifestFiles) {
  assert.match(
    lib,
    new RegExp(`plugin-manifests/${file.replace(".", "\\.")}`),
    `Rust runtime must consume ${file}`,
  );
}
assert.doesNotMatch(lib, /PluginDescriptor\s*\{/, "manual backend plugin descriptors are forbidden");

const kernelMod = await readFile(path.join(ROOT, "src-tauri", "src", "kernel", "mod.rs"), "utf8");
for (const deadModule of ["tab_host", "window_manager", "ipc_bridge", "shortcut_engine", "i18n_engine"]) {
  assert.doesNotMatch(kernelMod, new RegExp(`pub mod ${deadModule}`), `shadow kernel module ${deadModule} must stay removed`);
}

const workspace = await readFile(path.join(ROOT, "src", "context", "SplitLayoutContext.tsx"), "utf8");
assert.doesNotMatch(workspace, /localStorage/, "Workspace must persist through the backend ConfigStore");
assert.match(workspace, /workspace\.layout/, "Workspace backend key is missing");

const sendBarAssetFiles = [
  "CommandPanel.tsx",
  "AutoReplyPanel.tsx",
  "ScriptEditor.tsx",
  "SendBarContext.tsx",
];
for (const file of sendBarAssetFiles) {
  const source = await readFile(path.join(ROOT, "src", "components", "SendBar", file), "utf8");
  assert.doesNotMatch(source, /localStorage/, `${file}: engineering assets must persist through the backend ConfigStore`);
}
const commandPanel = await readFile(path.join(ROOT, "src", "components", "SendBar", "CommandPanel.tsx"), "utf8");
assert.match(commandPanel, /assets\.command_sets/, "Command asset store key is missing");


/* Pane UI source contract: source-level guard for the 1/2/2x2 responsive workspace.
 * This does not replace visual E2E; it prevents known structural regressions from
 * silently removing the responsive/scroll ownership rules that make small panes usable.
 */
const splitViewSource = await readFile(path.join(ROOT, "src", "components", "Layout", "SplitView.tsx"), "utf8");
const splitViewCss = await readFile(path.join(ROOT, "src", "components", "Layout", "SplitView.module.css"), "utf8");
assert.match(splitViewSource, /const PANE_HEADER_PX = 24;/, "Pane header/content inset geometry changed");
assert.match(splitViewCss, /\.paneSurface\s*\{[\s\S]*container-type:\s*size;[\s\S]*container-name:\s*session-pane;/, "Pane size container contract is missing");
assert.match(splitViewCss, /\.customPaneSurface\s*\{[\s\S]*overflow:\s*hidden;/, "Custom pane must not add a second same-axis scrollbar");
assert.match(splitViewCss, /\.paneHeader\s*\{[\s\S]*height:\s*23px;/, "Pane header geometry must remain aligned with the 24px content inset");

const tftpCss = await readFile(path.join(ROOT, "src", "components", "Tftp", "TftpSessionView.module.css"), "utf8");
assert.match(tftpCss, /\.container\s*\{[\s\S]*min-width:\s*0;[\s\S]*min-height:\s*0;[\s\S]*overflow:\s*hidden;/, "TFTP root must be pane-bounded");
assert.match(tftpCss, /@container session-pane \(max-width:\s*560px\)[\s\S]*\.container\s*\{[\s\S]*flex-direction:\s*column;[\s\S]*overflow-y:\s*auto;/, "TFTP narrow-pane controls must remain reachable");

const iperfCss = await readFile(path.join(ROOT, "src", "components", "Iperf", "IperfSessionView.module.css"), "utf8");
assert.match(iperfCss, /@container session-pane \(max-width:\s*640px\)[\s\S]*\.columns\s*\{[\s\S]*flex-direction:\s*column;/, "iperf narrow-pane layout must stack");
assert.match(iperfCss, /@container session-pane \(max-width:\s*440px\)[\s\S]*\.row2\s*\{[\s\S]*flex-direction:\s*column;/, "iperf compact form controls must remain reachable");

const trdpCss = await readFile(path.join(ROOT, "src", "plugins", "trdp", "TrdpSessionView.module.css"), "utf8");
assert.match(trdpCss, /\.navButton\s*\{[\s\S]*height:\s*36px;[\s\S]*min-height:\s*36px;[\s\S]*max-height:\s*36px;/, "TRDP top navigation geometry must be invariant");
assert.match(trdpCss, /\.content\s*\{[\s\S]*overflow:\s*auto;[\s\S]*scrollbar-gutter:\s*stable;/, "TRDP must own its internal scroll boundary");
assert.match(trdpCss, /@container session-pane \(max-width:\s*900px\)[\s\S]*\.analysisGrid\s*\{[\s\S]*grid-template-columns:\s*1fr;/, "TRDP Analysis must collapse to one column in narrow panes");

const networkCss = await readFile(path.join(ROOT, "src", "components", "Network", "NetworkDebugSessionView.module.css"), "utf8");
assert.match(networkCss, /\.dataArea\s*\{[\s\S]*min-height:\s*0;/, "Network Debug must stay pane-bounded");
assert.match(networkCss, /\.singleList\s*\{[\s\S]*overflow-y:\s*auto;[\s\S]*overflow-x:\s*hidden;/, "Network Debug stream must own its scroll boundary");

const persistence = await readFile(path.join(ROOT, "src-tauri", "src", "kernel", "persistence.rs"), "utf8");
assert.match(persistence, /AtomicWriteFile/, "TauTerm-owned state must keep the shared atomic persistence boundary");
assert.match(persistence, /\.commit\(\)/, "atomic persistence must commit only after the staged write succeeds");

const configStore = await readFile(path.join(ROOT, "src-tauri", "src", "kernel", "config_store.rs"), "utf8");
assert.match(configStore, /atomic_write\(&path, json\.as_bytes\(\)\)/, "ConfigStore must use atomic persistence");

const sessionStore = await readFile(path.join(ROOT, "src-tauri", "src", "kernel", "session_store.rs"), "utf8");
assert.match(sessionStore, /SESSION_LIBRARY_VERSION:\s*u32\s*=\s*1/, "Session Library must be versioned");
assert.match(sessionStore, /DEFAULT_MAX_ACTIVE_ROOT_SESSIONS:\s*usize\s*=\s*64/, "active root Session budget contract changed");
assert.match(sessionStore, /atomic_write\(path, json\.as_bytes\(\)\)/, "Session Library must use atomic persistence");
assert.doesNotMatch(
  sessionStore,
  /load_from_disk_unlocked\([^\n]+\)\.unwrap_or_default\(\)/,
  "Session Library read-modify-write must not overwrite state after a read error",
);

const logEngine = await readFile(path.join(ROOT, "src-tauri", "src", "kernel", "log_engine.rs"), "utf8");
assert.match(logEngine, /session_enabled/, "System and Session logging must have separate enable semantics");
assert.match(logEngine, /SESSION_LOG_ENABLED/, "disabled Session Log producers must stop before the shared queue");
assert.match(logEngine, /dropped_session_entries/, "Session log loss telemetry is missing");
assert.match(logEngine, /dropped_system_entries/, "System log loss telemetry is missing");

const knownHosts = await readFile(path.join(ROOT, "src-tauri", "src", "plugins", "ssh", "known_hosts.rs"), "utf8");
assert.match(knownHosts, /atomic_write\(&path, json\.as_bytes\(\)\)/, "SSH known-host trust must use atomic persistence");

const ssh = await readFile(path.join(ROOT, "src-tauri", "src", "plugins", "ssh", "mod.rs"), "utf8");
assert.match(ssh, /HostTrustDecision::Changed/, "SSH changed-host-key path must remain fail-closed");
assert.match(ssh, /ssh_trusted_connection/, "generic SSH ProtocolAdapter path must remain fail-closed");

console.log("product-integrity: canonical manifests, persistence, logging, and SSH trust contracts verified");
