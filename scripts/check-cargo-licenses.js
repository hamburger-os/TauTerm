#!/usr/bin/env node
import { spawnSync } from "node:child_process";

const result = spawnSync(
  "cargo",
  ["metadata", "--locked", "--format-version", "1", "--manifest-path", "src-tauri/Cargo.toml"],
  {
    encoding: "utf8",
    maxBuffer: 64 * 1024 * 1024,
    env: { ...process.env, CARGO_TERM_COLOR: "never" },
  },
);

if (result.error) throw result.error;
if (result.status !== 0) {
  process.stderr.write(result.stderr ?? "");
  process.exit(result.status ?? 1);
}

const metadata = JSON.parse(result.stdout);
const failures = [];
const copyleft = [];
const reviewedMpl = new Map([
  ["cssparser@0.36.0", "MPL-2.0"],
  ["cssparser-macros@0.6.1", "MPL-2.0"],
  ["dtoa-short@0.3.5", "MPL-2.0"],
  ["option-ext@0.2.0", "MPL-2.0"],
  ["selectors@0.36.1", "MPL-2.0"],
  ["serialport@4.10.0", "MPL-2.0"],
]);

function hasPermissiveOrChoice(expression) {
  if (!/\sOR\s/i.test(expression)) return false;
  return /\b(?:MIT|Apache-2\.0|BSD-2-Clause|BSD-3-Clause|ISC|Zlib|BSL-1\.0|CC0-1\.0|Unicode-3\.0)\b/i.test(expression);
}

for (const pkg of metadata.packages) {
  // Workspace package license is checked by scripts/check-third-party.js.
  if (pkg.name === "tauterm") continue;

  const license = pkg.license?.trim();
  if (!license && !pkg.license_file) {
    failures.push(pkg.name + " " + pkg.version + " has no license or license_file metadata");
    continue;
  }
  if (!license && pkg.license_file) {
    failures.push(pkg.name + " " + pkg.version + " uses license_file without an SPDX expression; review and record it explicitly");
    continue;
  }

  const expression = license;
  const packageKey = pkg.name + "@" + pkg.version;
  if (reviewedMpl.has(packageKey)) {
    if (expression !== reviewedMpl.get(packageKey)) {
      failures.push(packageKey + " license changed from reviewed value " + reviewedMpl.get(packageKey) + " to " + expression);
    }
    continue;
  }
  if (/\b(?:AGPL|GPL|LGPL|SSPL|MPL|EPL|CDDL)(?:-|\b)/i.test(expression) && !hasPermissiveOrChoice(expression)) {
    copyleft.push(pkg.name + " " + pkg.version + " = " + expression);
  }
}

if (copyleft.length) {
  failures.push(
    "Cargo dependency graph contains copyleft/restricted license expressions that require explicit repository review:\n    " +
      copyleft.join("\n    "),
  );
}

if (failures.length) {
  console.error("Cargo license metadata check failed:");
  for (const failure of failures) console.error("  - " + failure);
  process.exit(1);
}

const expressions = [...new Set(metadata.packages
  .filter((pkg) => pkg.name !== "tauterm")
  .map((pkg) => pkg.license ?? (pkg.license_file ? "license-file" : "unknown")))]
  .sort();

console.log("Cargo dependency license metadata present for " + (metadata.packages.length - 1) + " packages; " + reviewedMpl.size + " MPL packages match the reviewed allowlist.");
console.log("License expressions: " + expressions.join(", "));
