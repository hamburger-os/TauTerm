/**
 * ConPTY engine contract.
 *
 * Local Shell terminals must run on the reviewed Microsoft Windows Console
 * ConPTY redistributable, not on the in-box conhost engine: only the
 * redistributed engine forwards the mouse-tracking modes that VT-first
 * full-screen programs write before switching their console to raw input.
 *
 * This check keeps the vendored payload, its provenance, the Rust engine
 * selection and the Windows bundle mapping in one verified state, and keeps the
 * obsolete VT-input launcher from creeping back in.
 */
import { createHash } from "node:crypto";
import { existsSync, readFileSync, statSync } from "node:fs";
import { resolve } from "node:path";

const root = resolve(import.meta.dirname, "..");
const engineVersion = "1.25.260930003";
const vendorDir = resolve(root, "src-tauri", "vendor", "conpty", engineVersion);

const failures = [];
const fail = (message) => failures.push(message);
const read = (relativePath) => readFileSync(resolve(root, relativePath), "utf8");
const readJson = (relativePath) => JSON.parse(read(relativePath));

const payload = [
  {
    file: "conpty.dll",
    bytes: 119608,
    sha256: "FEEEF341D891643C62D30B6B07800BC70F0BC148F44CB8C3BEE84AA557AE805A",
  },
  {
    file: "OpenConsole.exe",
    bytes: 1089376,
    sha256: "3D66B23D0A71BB8EED2B77EDC8B9DF9BF54CE6C8FB74C863A30E760997F80586",
  },
];

for (const entry of payload) {
  const path = resolve(vendorDir, "win-x64", entry.file);
  if (!existsSync(path)) {
    fail(`missing vendored engine file: ${path}`);
    continue;
  }
  const actualBytes = statSync(path).size;
  if (actualBytes !== entry.bytes) {
    fail(`${entry.file} size changed: expected ${entry.bytes}, found ${actualBytes}`);
  }
  const actualSha256 = createHash("sha256").update(readFileSync(path)).digest("hex").toUpperCase();
  if (actualSha256 !== entry.sha256) {
    fail(`${entry.file} SHA-256 mismatch: expected ${entry.sha256}, found ${actualSha256}`);
  }
}

if (!existsSync(resolve(vendorDir, "LICENSE"))) {
  fail("vendored ConPTY payload is missing its MIT license file");
}
const provenance = read(`src-tauri/vendor/conpty/${engineVersion}/README.md`);
if (!provenance.includes(`Microsoft.Windows.Console.ConPTY`) || !provenance.includes(engineVersion)) {
  fail("vendor README must record the originating NuGet package and exact version");
}
for (const entry of payload) {
  if (!provenance.includes(entry.sha256)) {
    fail(`vendor README must record the reviewed SHA-256 for ${entry.file}`);
  }
}

const engineSource = read("src-tauri/src/plugins/local_shell/conpty_engine.rs");
if (!engineSource.includes(`ENGINE_VERSION: &str = "${engineVersion}"`)) {
  fail(`conpty_engine.rs must pin ENGINE_VERSION = "${engineVersion}"`);
}

const windowsConfig = readJson("src-tauri/tauri.windows.conf.json");
const resources = windowsConfig.bundle?.resources ?? {};
const expectedResources = {
  [`vendor/conpty/${engineVersion}/win-x64/conpty.dll`]: "conpty/win-x64/conpty.dll",
  [`vendor/conpty/${engineVersion}/win-x64/OpenConsole.exe`]: "conpty/win-x64/OpenConsole.exe",
  [`vendor/conpty/${engineVersion}/LICENSE`]: "THIRD_PARTY_CONPTY_MIT.txt",
};
for (const [sourcePath, targetPath] of Object.entries(expectedResources)) {
  if (resources[sourcePath] !== targetPath) {
    fail(`Windows bundle resource mismatch: ${sourcePath} -> ${targetPath}`);
  }
}

const driverSource = read("src-tauri/src/plugins/local_shell/driver.rs");
if (!driverSource.includes("conpty_engine::install()")) {
  fail("LocalShellDriver::spawn must pin the bundled ConPTY engine before opening the PTY");
}
if (!driverSource.includes("CONPTY_STARTUP_QUERIES")) {
  fail("LocalShellDriver must answer the ConPTY startup handshake from the engine");
}
// portable-pty resolves `conpty.dll` lazily on the first `openpty` and caches it
// for the process, so pinning the engine afterwards would silently keep using the
// in-box engine. Enforce the ordering inside `spawn`.
const spawnStart = driverSource.indexOf("pub fn spawn(");
const spawnBody = spawnStart >= 0 ? driverSource.slice(spawnStart, spawnStart + 4000) : "";
const installOffset = spawnBody.indexOf("conpty_engine::install()");
const openPtyOffset = spawnBody.indexOf(".openpty(");
if (installOffset < 0 || openPtyOffset < 0 || installOffset > openPtyOffset) {
  fail("ConPTY engine must be pinned before portable-pty's first .openpty() resolves the engine");
}

if (existsSync(resolve(root, "src-tauri/src/plugins/local_shell/console_vt_input.rs"))) {
  fail("obsolete ConPTY VT-input launcher module is back; the engine selection replaces it");
}
if (read("src-tauri/src/plugins/local_shell/mod.rs").includes("console_vt_input")) {
  fail("local_shell must not declare the obsolete ConPTY launcher module");
}

const notices = read("THIRD_PARTY_LICENSES.md");
if (!notices.includes("Microsoft Windows Console ConPTY")) {
  fail("THIRD_PARTY_LICENSES.md must inventory the redistributed ConPTY engine");
}

if (failures.length > 0) {
  console.error("ConPTY engine contract failed:");
  for (const failure of failures) console.error(`  - ${failure}`);
  process.exit(1);
}
console.log(`✅ ConPTY engine ${engineVersion} is vendored, verified and bundled.`);
