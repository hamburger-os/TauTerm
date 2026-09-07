#!/usr/bin/env node
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.join(path.dirname(fileURLToPath(import.meta.url)), ".."));
const failures = [];

function fail(message) { failures.push(message); }
function exists(rel) { return fs.existsSync(path.join(root, rel)); }
function read(rel) { return fs.readFileSync(path.join(root, rel), "utf8"); }
function json(rel) { return JSON.parse(read(rel)); }

const required = [
  "LICENSE",
  "LICENSE-APACHE",
  "THIRD_PARTY_LICENSES.md",
  "resources/com0com/README.md",
  "resources/com0com/SOURCE.md",
  "resources/com0com/COPYING-GPL-2.0.txt",
  "src-tauri/vendor/tcnopen/LICENSE",
  "src-tauri/vendor/tcnopen/README.md",
  "src-tauri/vendor/tcnopen/SOURCE.json",
  "src-tauri/vendor/riperf3/LICENSE-MIT.txt",
  "src-tauri/vendor/riperf3/LICENSE-APACHE.txt",
  "src-tauri/vendor/riperf3/VENDOR-NOTES.md",
  "scripts/generate-third-party-notices.js",
];

for (const rel of required) {
  if (!exists(rel)) fail("missing third-party/license contract file: " + rel);
}

const pkg = json("package.json");
if (pkg.license !== "MIT OR Apache-2.0") {
  fail('package.json license must be "MIT OR Apache-2.0"');
}

const cargoToml = read("src-tauri/Cargo.toml");
if (!/^license\s*=\s*"MIT OR Apache-2\.0"$/m.test(cargoToml)) {
  fail('src-tauri/Cargo.toml license must be "MIT OR Apache-2.0"');
}

const notice = read("THIRD_PARTY_LICENSES.md");
for (const marker of ["com0com 3.0.0.0", "TCNOpen TRDP 3.0.0.0", "riperf3 0.8.0", "Lua 5.4", "serialport 4.10.0", "THIRD_PARTY_DEPENDENCY_LICENSES.txt", "Npcap", "libpcap"]) {
  if (!notice.includes(marker)) fail("THIRD_PARTY_LICENSES.md missing inventory marker: " + marker);
}

const comSource = read("resources/com0com/SOURCE.md");
if (!comSource.includes("GPL-2.0-or-later") || !comSource.includes("com0com-3.0.0.0.zip")) {
  fail("com0com SOURCE.md must record GPL-2.0-or-later and the corresponding 3.0.0.0 source archive");
}
const gpl = read("resources/com0com/COPYING-GPL-2.0.txt");
if (!gpl.startsWith("GNU GENERAL PUBLIC LICENSE\nVersion 2, June 1991")) {
  fail("com0com GPL version 2 license text is missing or unexpected");
}

const source = json("src-tauri/vendor/tcnopen/SOURCE.json");
if (source.version !== "3.0.0.0" || source.license !== "MPL-2.0") {
  fail("TCNOpen SOURCE.json version/license metadata is unexpected");
}
for (const patch of source.downstream_patches ?? []) {
  const rel = path.posix.join("src-tauri/vendor/tcnopen", patch.file);
  if (!exists(rel)) fail("TCNOpen SOURCE.json references missing downstream patch: " + rel);
}
if ((source.downstream_patches ?? []).length === 0) {
  fail("TCNOpen downstream patch series unexpectedly empty; review provenance before changing this check");
}
if (read("src-tauri/vendor/tcnopen/README.md").includes("does not modify these upstream source files")) {
  fail("TCNOpen README incorrectly claims the vendored tree is unmodified");
}

const riperfCargo = read("src-tauri/vendor/riperf3/Cargo.toml");
if (!/^version\s*=\s*"0\.8\.0"$/m.test(riperfCargo) || !/^license\s*=\s*"MIT OR Apache-2\.0"$/m.test(riperfCargo)) {
  fail("vendored riperf3 version/license metadata is unexpected");
}

if (!/mlua\s*=\s*\{[^\n]*features\s*=\s*\[[^\]]*"lua54"[^\]]*"vendored"/m.test(cargoToml)) {
  fail("Cargo.toml no longer matches the reviewed vendored Lua 5.4 configuration");
}
const cargoLock = read("src-tauri/Cargo.lock");
if (!/name = "lua-src"[\s\S]*?version = "547\.0\.0"/m.test(cargoLock)) {
  fail("Cargo.lock Lua source version changed; re-review Lua runtime notice");
}

const vendorRoot = path.join(root, "src-tauri/vendor");
const vendorDirs = fs.readdirSync(vendorRoot, { withFileTypes: true })
  .filter((entry) => entry.isDirectory())
  .map((entry) => entry.name)
  .sort();
const reviewedVendorDirs = ["riperf3", "tcnopen"];
for (const name of vendorDirs) {
  if (!reviewedVendorDirs.includes(name)) {
    fail("unreviewed vendored dependency directory: src-tauri/vendor/" + name);
  }
}
for (const name of reviewedVendorDirs) {
  if (!vendorDirs.includes(name)) fail("reviewed vendor directory unexpectedly missing: " + name);
}

const packageLock = json("package-lock.json");
for (const [packagePath, metadata] of Object.entries(packageLock.packages ?? {})) {
  if (!packagePath) continue;
  if (!metadata.license) fail("npm lock entry has no license metadata: " + packagePath);
  const expression = String(metadata.license ?? "");
  if (/SEE LICENSE IN|UNLICENSED|UNKNOWN/i.test(expression)) {
    fail("npm dependency license metadata requires explicit manual review: " + packagePath + " = " + expression);
  }
  if (/\b(?:AGPL|GPL|LGPL|SSPL|MPL|EPL|CDDL)(?:-|\b)/i.test(expression)) {
    fail("npm dependency introduced a copyleft/restricted license requiring manual review: " + packagePath + " = " + expression);
  }
}

const baseConfig = json("src-tauri/tauri.conf.json");
const windowsConfig = json("src-tauri/tauri.windows.conf.json");
const commonBundleResources = {
  "../LICENSE": "LICENSE-MIT",
  "../LICENSE-APACHE": "LICENSE-APACHE",
  "../THIRD_PARTY_LICENSES.md": "THIRD_PARTY_LICENSES.md",
  "vendor/tcnopen/LICENSE": "THIRD_PARTY_TCNOPEN_MPL-2.0.txt",
  "generated/THIRD_PARTY_DEPENDENCY_LICENSES.txt": "THIRD_PARTY_DEPENDENCY_LICENSES.txt",
};
for (const [label, config] of [["base", baseConfig], ["windows", windowsConfig]]) {
  const resources = config.bundle?.resources ?? {};
  for (const [sourcePath, targetPath] of Object.entries(commonBundleResources)) {
    if (resources[sourcePath] !== targetPath) {
      fail(label + " Tauri bundle resource mismatch: " + sourcePath + " -> " + targetPath);
    }
  }
}
if (baseConfig.build?.beforeBundleCommand !== "node scripts/generate-third-party-notices.js && node scripts/prepare-service-bin.js") {
  fail("beforeBundleCommand must generate dependency notices before staging native binaries");
}
if (!Object.prototype.hasOwnProperty.call(windowsConfig.bundle?.resources ?? {}, "../resources/com0com/*")) {
  fail("Windows bundle must include the reviewed com0com distribution payload");
}
const releaseWorkflow = read(".github/workflows/release.yml");
for (const marker of [
  "com0com-3.0.0.0.zip/download",
  "com0com-3.0.0.0-source.zip",
  "unzip -t",
]) {
  if (!releaseWorkflow.includes(marker)) {
    fail("release workflow is missing com0com corresponding-source contract marker: " + marker);
  }
}

if (failures.length) {
  console.error("Third-party license contract failed:");
  for (const item of failures) console.error("  - " + item);
  process.exit(1);
}

console.log("Third-party license contract is consistent.");
