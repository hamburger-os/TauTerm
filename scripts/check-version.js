import { existsSync, readFileSync } from "node:fs";
import { resolve } from "node:path";

const root = resolve(import.meta.dirname, "..");
const args = process.argv.slice(2);
const releaseMode = args.includes("--release");
const explicitVersion = args.find((arg) => !arg.startsWith("--"));

function readJson(path) {
  return JSON.parse(readFileSync(resolve(root, path), "utf8"));
}

function fail(message) {
  console.error(`❌ ${message}`);
  process.exitCode = 1;
}

function same(label, actual, expected) {
  if (actual !== expected) {
    fail(`${label}: expected ${expected}, found ${actual}`);
  } else {
    console.log(`✅ ${label}: ${actual}`);
  }
}

const pkg = readJson("package.json");
const expected = (explicitVersion ?? pkg.version).replace(/^v/, "");

if (!/^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z.-]+)?$/.test(expected)) {
  fail(`invalid semantic version: ${expected}`);
  process.exit(1);
}

same("package.json", pkg.version, expected);

const lock = readJson("package-lock.json");
same("package-lock.json", lock.version, expected);
same('package-lock.json packages[""]', lock.packages?.[""]?.version, expected);

const tauri = readJson("src-tauri/tauri.conf.json");
same("src-tauri/tauri.conf.json", tauri.version, expected);

const cargo = readFileSync(resolve(root, "src-tauri/Cargo.toml"), "utf8");
const cargoVersion = cargo.match(/^\[package\][\s\S]*?^version\s*=\s*"([^"]+)"/m)?.[1];
if (!cargoVersion) {
  fail("src-tauri/Cargo.toml: package version not found");
} else {
  same("src-tauri/Cargo.toml", cargoVersion, expected);
}

if (releaseMode) {
  const changelog = readFileSync(resolve(root, "CHANGELOG.md"), "utf8");
  const escaped = expected.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  const changelogHeading = new RegExp(`^## \\[${escaped}\\](?:\\s|$)`, "m");
  if (!changelogHeading.test(changelog)) {
    fail(`CHANGELOG.md: missing release heading for ${expected}`);
  } else {
    console.log(`✅ CHANGELOG.md: release ${expected} is documented`);
  }

  const notesPath = resolve(root, `docs/RELEASE_NOTES_v${expected}.md`);
  if (!existsSync(notesPath)) {
    fail(`missing docs/RELEASE_NOTES_v${expected}.md`);
  } else {
    const notes = readFileSync(notesPath, "utf8").trim();
    if (notes.length < 80 || !notes.includes(`v${expected}`)) {
      fail(`docs/RELEASE_NOTES_v${expected}.md: release notes are empty or do not identify v${expected}`);
    } else {
      console.log(`✅ docs/RELEASE_NOTES_v${expected}.md`);
    }
  }
}

if (process.exitCode) {
  process.exit(process.exitCode);
}

console.log(`🎉 Version metadata is consistent for ${expected}${releaseMode ? " and release metadata is ready" : ""}.`);
