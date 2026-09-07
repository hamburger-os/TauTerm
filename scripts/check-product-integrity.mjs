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

const sessionStore = await readFile(path.join(ROOT, "src-tauri", "src", "kernel", "session_store.rs"), "utf8");
assert.match(sessionStore, /SESSION_LIBRARY_VERSION:\s*u32\s*=\s*1/, "Session Library must be versioned");
assert.match(sessionStore, /DEFAULT_MAX_ACTIVE_ROOT_SESSIONS:\s*usize\s*=\s*64/, "active root Session budget contract changed");

const logEngine = await readFile(path.join(ROOT, "src-tauri", "src", "kernel", "log_engine.rs"), "utf8");
assert.match(logEngine, /session_enabled/, "System and Session logging must have separate enable semantics");
assert.match(logEngine, /dropped_session_entries/, "Session log loss telemetry is missing");
assert.match(logEngine, /dropped_system_entries/, "System log loss telemetry is missing");

const ssh = await readFile(path.join(ROOT, "src-tauri", "src", "plugins", "ssh", "mod.rs"), "utf8");
assert.match(ssh, /HostTrustDecision::Changed/, "SSH changed-host-key path must remain fail-closed");
assert.match(ssh, /ssh_trusted_connection/, "generic SSH ProtocolAdapter path must remain fail-closed");

console.log("product-integrity: canonical manifests, persistence, logging, and SSH trust contracts verified");
