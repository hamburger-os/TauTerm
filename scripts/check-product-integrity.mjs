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

  const frontendExt = ["modbus", "network", "trdp"].includes(manifest.id) ? "tsx" : "ts";
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
