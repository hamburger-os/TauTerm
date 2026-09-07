#!/usr/bin/env node
import { createHash } from "node:crypto";
import { spawnSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.join(path.dirname(fileURLToPath(import.meta.url)), ".."));
const output = path.join(root, "src-tauri", "generated", "THIRD_PARTY_DEPENDENCY_LICENSES.txt");
const MAX_LICENSE_BYTES = 1024 * 1024;
const licenseName = /^(?:licen[cs]e|copying|notice)(?:[._-].*)?$/i;

function normalize(text) {
  return text.replace(/\r\n/g, "\n").replace(/\r/g, "\n").trimEnd() + "\n";
}

function readLicenseFile(file) {
  try {
    const stat = fs.statSync(file);
    if (!stat.isFile() || stat.size <= 0 || stat.size > MAX_LICENSE_BYTES) return null;
    const text = fs.readFileSync(file, "utf8");
    if (text.includes("\0")) return null;
    return normalize(text);
  } catch {
    return null;
  }
}

function candidateLicenseFiles(dir, explicit = null) {
  const found = new Map();
  if (explicit) {
    const explicitPath = path.isAbsolute(explicit) ? explicit : path.resolve(dir, explicit);
    const text = readLicenseFile(explicitPath);
    if (text) found.set(path.basename(explicitPath), text);
  }
  try {
    for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
      if (!entry.isFile() || !licenseName.test(entry.name)) continue;
      const text = readLicenseFile(path.join(dir, entry.name));
      if (text) found.set(entry.name, text);
    }
  } catch {
    // Platform-specific optional dependencies may be locked but not installed.
  }
  return [...found.entries()].sort(([a], [b]) => a.localeCompare(b));
}

function cargoPackages() {
  const result = spawnSync(
    "cargo",
    ["metadata", "--locked", "--format-version", "1", "--manifest-path", "src-tauri/Cargo.toml"],
    {
      cwd: root,
      encoding: "utf8",
      maxBuffer: 64 * 1024 * 1024,
      env: { ...process.env, CARGO_TERM_COLOR: "never" },
    },
  );
  if (result.error) throw result.error;
  if (result.status !== 0) {
    process.stderr.write(result.stderr ?? "");
    throw new Error("cargo metadata failed while generating third-party notices");
  }

  const metadata = JSON.parse(result.stdout);
  return metadata.packages
    .filter((pkg) => pkg.name !== "tauterm")
    .map((pkg) => {
      const dir = path.dirname(pkg.manifest_path);
      return {
        ecosystem: "Cargo",
        name: pkg.name,
        version: pkg.version,
        license: pkg.license ?? (pkg.license_file ? "custom license file" : "UNKNOWN"),
        source: pkg.source?.startsWith("registry+") ? "crates.io registry" : (pkg.repository ?? "path dependency"),
        files: candidateLicenseFiles(dir, pkg.license_file),
      };
    });
}

function npmPackages() {
  const lock = JSON.parse(fs.readFileSync(path.join(root, "package-lock.json"), "utf8"));
  return Object.entries(lock.packages ?? {})
    .filter(([packagePath, meta]) => packagePath && meta?.version)
    .map(([packagePath, meta]) => ({
      ecosystem: "npm",
      name: packagePath.replace(/^.*node_modules\//, ""),
      version: String(meta.version),
      license: String(meta.license ?? "UNKNOWN"),
      source: meta.resolved ? "npm registry" : "package-lock dependency",
      files: candidateLicenseFiles(path.join(root, packagePath)),
    }));
}

const packages = [...cargoPackages(), ...npmPackages()].sort((a, b) =>
  a.ecosystem.localeCompare(b.ecosystem) ||
  a.name.localeCompare(b.name) ||
  a.version.localeCompare(b.version)
);

const blocks = new Map();
for (const pkg of packages) {
  pkg.licenseIds = [];
  for (const [filename, text] of pkg.files) {
    const hash = createHash("sha256").update(text, "utf8").digest("hex");
    const id = hash.slice(0, 16);
    pkg.licenseIds.push(filename + ":" + id);
    if (!blocks.has(hash)) blocks.set(hash, { id, text, packages: [] });
    blocks.get(hash).packages.push(pkg.ecosystem + ":" + pkg.name + "@" + pkg.version + " (" + filename + ")");
  }
}

const lines = [
  "TauTerm Generated Third-Party Dependency Notices",
  "================================================",
  "",
  "This file is generated from the exact Cargo metadata and package-lock dependency graph used for the build.",
  "It intentionally includes build/development dependencies as a conservative attribution set; presence here does not imply a package is linked into every TauTerm binary.",
  "Special redistributed/vendored components and their distribution obligations are documented separately in THIRD_PARTY_LICENSES.md.",
  "",
  "Dependency inventory",
  "--------------------",
];

for (const pkg of packages) {
  lines.push(
    pkg.ecosystem + " | " + pkg.name + " | " + pkg.version + " | " + pkg.license + " | " + pkg.source +
      (pkg.licenseIds.length ? " | texts=" + pkg.licenseIds.join(",") : " | texts=not-present-in-installed-package"),
  );
}

lines.push("", "License / notice texts", "----------------------", "");
for (const [hash, block] of [...blocks.entries()].sort(([a], [b]) => a.localeCompare(b))) {
  lines.push(
    "----- " + block.id + " (sha256 " + hash + ") -----",
    "Packages: " + [...new Set(block.packages)].sort().join(", "),
    "",
    block.text.trimEnd(),
    "",
  );
}

fs.mkdirSync(path.dirname(output), { recursive: true });
fs.writeFileSync(output, lines.join("\n") + "\n", "utf8");
console.log("Generated " + path.relative(root, output) + " for " + packages.length + " dependency packages and " + blocks.size + " unique license/notice texts.");
