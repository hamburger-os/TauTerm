import assert from "node:assert/strict";
import { access, readFile, readdir } from "node:fs/promises";
import path from "node:path";

const ROOT = process.cwd();
const manifestDir = path.join(ROOT, "src", "plugin-manifests");
const manifestFiles = (await readdir(manifestDir))
  .filter(name => name.endsWith(".json"))
  .sort();

async function exists(target) {
  try {
    await access(target);
    return true;
  } catch {
    return false;
  }
}

assert.deepEqual(
  manifestFiles,
  [
    "iperf.json",
    "local-shell.json",
    "modbus.json",
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
const frontendCatalog = await readFile(path.join(ROOT, "src", "plugins", "catalog.ts"), "utf8");
const backendCatalog = await readFile(
  path.join(ROOT, "src-tauri", "src", "plugins", "catalog.rs"),
  "utf8",
);

for (const file of manifestFiles) {
  const manifest = JSON.parse(await readFile(path.join(manifestDir, file), "utf8"));
  for (const field of requiredStringFields) {
    assert.equal(typeof manifest[field], "string", `${file}: ${field} must be a string`);
    assert.ok(manifest[field].length > 0, `${file}: ${field} must not be empty`);
  }
  assert.ok(
    ["terminal", "custom"].includes(manifest.content_type),
    `${file}: content_type must be a supported canonical renderer class`,
  );
  assert.equal(typeof manifest.send_bar, "boolean", `${file}: send_bar must be boolean`);
  assert.ok(Array.isArray(manifest.capabilities), `${file}: capabilities must be an array`);
  assert.ok(Array.isArray(manifest.transfer_protocols), `${file}: transfer_protocols must be an array`);
  assert.equal(path.basename(file, ".json"), manifest.id, `${file}: filename and id must match`);
  assert.ok(!ids.has(manifest.id), `${file}: duplicate plugin id ${manifest.id}`);
  ids.add(manifest.id);

  const frontendExt = ["modbus", "network", "trdp"].includes(manifest.id) ? "tsx" : "ts";
  const frontend = await readFile(
    path.join(ROOT, "src", "plugins", manifest.id, `index.${frontendExt}`),
    "utf8",
  );
  assert.match(
    frontend,
    new RegExp(`plugin-manifests/${manifest.id}\\.json`),
    `${manifest.id}: frontend definition must consume canonical manifest`,
  );
  assert.match(
    frontend,
    /definePlugin\s*\(/,
    `${manifest.id}: frontend module must export an inert plugin definition`,
  );
  assert.doesNotMatch(
    frontend,
    /manifest:\s*\{/,
    `${manifest.id}: inline manifest duplicates are forbidden`,
  );
  assert.doesNotMatch(
    frontend,
    /registerPlugin\s*\(/,
    `${manifest.id}: frontend plugin modules must not mutate the registry at import time`,
  );
  assert.match(
    frontendCatalog,
    new RegExp(`from ["']\\./${manifest.id}["']`),
    `${manifest.id}: frontend catalog must include the built-in plugin`,
  );
  assert.match(
    backendCatalog,
    new RegExp(`plugin-manifests/${file.replace(".", "\\.")}`),
    `Rust plugin catalog must consume ${file}`,
  );
}

const main = await readFile(path.join(ROOT, "src", "main.tsx"), "utf8");
assert.match(main, /installBuiltinPlugins\(\)/, "main.tsx must install the explicit frontend plugin catalog");
assert.doesNotMatch(
  main,
  /import\s+["']\.\/plugins\/(?:serial|ssh|telnet|local-shell|tftp|iperf|network|modbus|trdp)["']/,
  "main.tsx must not register concrete plugins through side-effect imports",
);

const lib = await readFile(path.join(ROOT, "src-tauri", "src", "lib.rs"), "utf8");
assert.match(lib, /plugins::catalog::build_runtime\(\)/, "Rust bootstrap must install the backend plugin catalog");
assert.match(lib, /tauterm_invoke_handler!/, "Rust bootstrap must delegate plugin IPC registration to the backend catalog");
assert.doesNotMatch(lib, /PluginDescriptor\s*\{/, "manual backend plugin descriptors are forbidden");
for (const adapter of ["SerialAdapter", "SshAdapter", "TelnetAdapter", "TftpAdapter", "IperfAdapter", "NetworkAdapter", "ModbusAdapter"]) {
  assert.doesNotMatch(lib, new RegExp(`${adapter}::new`), `lib.rs must not assemble ${adapter} directly`);
}
for (const plugin of ["serial", "ssh", "telnet", "local_shell", "tftp", "iperf", "network", "modbus", "trdp"]) {
  assert.doesNotMatch(
    lib,
    new RegExp(`plugins::${plugin}::`),
    `lib.rs must not register concrete plugin IPC directly: ${plugin}`,
  );
}

const pluginAdapter = await readFile(
  path.join(ROOT, "src-tauri", "src", "kernel", "plugin_adapter.rs"),
  "utf8",
);
assert.doesNotMatch(
  pluginAdapter,
  /fn\s+content_type\s*\(&self\)/,
  "ProtocolAdapter must not duplicate canonical manifest content_type",
);

const commands = await readFile(path.join(ROOT, "src-tauri", "src", "commands.rs"), "utf8");
assert.match(
  commands,
  /pub transfer_enabled: bool,/,
  "saved-session IPC must require an explicit transfer_enabled value",
);
assert.match(
  commands,
  /pub send_bar_enabled: bool,/,
  "saved-session IPC must require an explicit send_bar_enabled value",
);
assert.doesNotMatch(
  commands,
  /transfer_enabled\.unwrap_or\(true\)|send_bar_enabled\.unwrap_or\(true\)/,
  "generic saved-session IPC must not invent plugin UI capability defaults",
);

for (const legacyUiDir of ["Tftp", "Iperf"]) {
  assert.equal(
    await exists(path.join(ROOT, "src", "components", legacyUiDir)),
    false,
    `${legacyUiDir} private UI must live inside its plugin directory`,
  );
}
assert.equal(
  await exists(path.join(ROOT, "src", "plugins", "tftp", "TftpSessionView.tsx")),
  true,
  "TFTP custom view must be plugin-owned",
);
assert.equal(
  await exists(path.join(ROOT, "src", "plugins", "iperf", "IperfSessionView.tsx")),
  true,
  "iperf custom view must be plugin-owned",
);

const kernelMod = await readFile(path.join(ROOT, "src-tauri", "src", "kernel", "mod.rs"), "utf8");
for (const deadModule of ["tab_host", "window_manager", "ipc_bridge", "shortcut_engine", "i18n_engine"]) {
  assert.doesNotMatch(kernelMod, new RegExp(`pub mod ${deadModule}`), `shadow kernel module ${deadModule} must stay removed`);
}
