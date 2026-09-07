#!/usr/bin/env node
/**
 * TauTerm documentation contract checker.
 *
 * Canonical policy lives in AGENTS.md and .agents/skills/tauterm-docs/SKILL.md.
 * This script only enforces rules that can be checked mechanically.
 */
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.resolve(
  process.argv.includes("--root")
    ? process.argv[process.argv.indexOf("--root") + 1]
    : path.join(path.dirname(fileURLToPath(import.meta.url)), ".."),
);

const required = [
  "AGENTS.md",
  "README.md",
  "README.zh-CN.md",
  "CONTRIBUTING.md",
  "CHANGELOG.md",
  ".agents/skills/tauterm-docs/SKILL.md",
  "docs/README.md",
  "docs/community/BUILDING.md",
  "docs/community/RELEASING.md",
  "docs/community/SUPPORTED_PLATFORMS.md",
  "docs/product/PRODUCT_STRATEGY.md",
  "docs/product/HARDWARE_ECOSYSTEM.md",
  "docs/product/COMMERCIALIZATION.md",
  "docs/modules/CORE.md",
  "docs/modules/WORKSPACE.md",
  "docs/modules/SERIAL.md",
  "docs/modules/SSH.md",
  "docs/modules/LOCAL_SHELL.md",
  "docs/modules/NETWORK.md",
  "docs/modules/TRDP.md",
  "docs/modules/AUTOMATION.md",
  "docs/modules/PLATFORM_SECURITY.md",
];

const forbiddenLegacy = [
  "docs/ARCHITECTURE.md",
  "docs/BUILDING.md",
  "docs/RELEASING.md",
  "docs/SUPPORTED_PLATFORMS.md",
  "docs/PRODUCT_STRATEGY.md",
  "docs/HARDWARE_ECOSYSTEM.md",
  "docs/COMMERCIALIZATION.md",
  "docs/SPLIT_VIEW_DESIGN.md",
];

const errors = [];
const warnings = [];
const checks = [];

function pass(name) {
  checks.push({ name, ok: true });
}

function fail(name, message) {
  checks.push({ name, ok: false });
  errors.push(message);
}

function warn(message) {
  warnings.push(message);
}

function exists(rel) {
  return fs.existsSync(path.join(root, rel));
}

function read(rel) {
  return fs.readFileSync(path.join(root, rel), "utf8");
}

function walk(dir, filter = () => true) {
  const abs = path.join(root, dir);
  if (!fs.existsSync(abs)) return [];
  const out = [];
  for (const entry of fs.readdirSync(abs, { withFileTypes: true })) {
    const rel = path.posix.join(dir.replaceAll("\\", "/"), entry.name);
    if (entry.isDirectory()) out.push(...walk(rel, filter));
    else if (filter(rel)) out.push(rel);
  }
  return out;
}

function markdownFiles() {
  const rootDocs = [
    "AGENTS.md",
    "README.md",
    "README.zh-CN.md",
    "CONTRIBUTING.md",
    "SECURITY.md",
    "CHANGELOG.md",
    "THIRD_PARTY_LICENSES.md",
  ].filter(exists);
  const docs = walk("docs", (rel) => rel.endsWith(".md"));
  const skills = walk(".agents/skills", (rel) => rel.endsWith("/SKILL.md"));
  return [...new Set([...rootDocs, ...docs, ...skills])];
}

function checkRequiredFiles() {
  const missing = required.filter((rel) => !exists(rel));
  if (missing.length) fail("required files", "Missing canonical documentation: " + missing.join(", "));
  else pass("required files");
}

function checkLegacyFiles() {
  const legacy = forbiddenLegacy.filter(exists);
  const releaseNotes = walk("docs", (rel) => /RELEASE_NOTES_v.+\.md$/i.test(rel));
  const found = [...legacy, ...releaseNotes];
  if (found.length) {
    fail(
      "legacy duplicates",
      "Legacy/duplicated documentation must be removed: " + found.join(", "),
    );
  } else {
    pass("legacy duplicates");
  }
}

function headings(md) {
  return (md.match(/^##\s+(.+)$/gm) || []).map((line) => line.replace(/^##\s+/, "").trim());
}

function checkReadmes() {
  const en = read("README.md");
  const zh = read("README.zh-CN.md");
  const enHeadings = headings(en);
  const zhHeadings = headings(zh);

  if (enHeadings.length !== zhHeadings.length) {
    fail(
      "README mirror",
      "README heading count differs: README.md=" +
        enHeadings.length +
        ", README.zh-CN.md=" +
        zhHeadings.length,
    );
  } else {
    pass("README mirror");
  }

  for (const heading of zhHeadings) {
    if (!/\p{Script=Han}/u.test(heading)) {
      warn("README.zh-CN.md heading may be untranslated: " + heading);
    }
  }

  const banned = /MobaXterm|WindTerm|VOFA\+?|Tabby|Electron/i;
  const hits = [];
  for (const rel of ["README.md", "README.zh-CN.md"]) {
    if (banned.test(read(rel))) hits.push(rel);
  }
  if (hits.length) fail("README neutrality", "Named competitor/comparison terms found in: " + hits.join(", "));
  else pass("README neutrality");

  for (const rel of ["README.md", "README.zh-CN.md"]) {
    const lines = read(rel).split("\n").length;
    if (lines > 320) warn(rel + " is " + lines + " lines; keep the public landing page concise.");
  }
}

function linkTarget(raw) {
  const target = raw.trim().replace(/^<|>$/g, "");
  if (!target || target.startsWith("#")) return null;
  if (/^(https?:|mailto:|tel:)/i.test(target)) return null;
  const withoutTitle = target.split(/\s+["']/)[0];
  return withoutTitle.split("#")[0].split("?")[0];
}

function checkLinks() {
  const broken = [];
  const linkRe = /\[[^\]]*\]\(([^)]+)\)/g;

  for (const rel of markdownFiles()) {
    const md = read(rel);
    let match;
    while ((match = linkRe.exec(md)) !== null) {
      const target = linkTarget(match[1]);
      if (!target) continue;
      const decoded = decodeURIComponent(target);
      const resolved = path.resolve(path.dirname(path.join(root, rel)), decoded);
      if (!fs.existsSync(resolved)) broken.push(rel + " -> " + target);
    }
  }

  if (broken.length) fail("relative links", "Broken relative Markdown links:\n  " + broken.join("\n  "));
  else pass("relative links");
}

function checkOwnerLanguage() {
  const ownerDocs = [
    "docs/README.md",
    ...walk("docs/modules", (rel) => rel.endsWith(".md")),
    ...walk("docs/product", (rel) => rel.endsWith(".md")),
  ];
  const wrong = ownerDocs.filter((rel) => !/\p{Script=Han}/u.test(read(rel)));
  if (wrong.length) fail("maintainer language", "Maintainer documents must be Chinese-first: " + wrong.join(", "));
  else pass("maintainer language");
}

function checkChangelog() {
  const changelog = read("CHANGELOG.md");
  const okFormat =
    changelog.includes("Keep a Changelog") &&
    /^##\s+\[Unreleased\]/m.test(changelog) &&
    /^##\s+\[\d+\.\d+\.\d+(?:-[^\]]+)?\]/m.test(changelog);
  if (!okFormat) fail("CHANGELOG", "CHANGELOG.md does not match the repository release-history contract.");
  else pass("CHANGELOG");
}

function flatten(obj, prefix = "") {
  const out = [];
  for (const [key, value] of Object.entries(obj)) {
    const full = prefix ? prefix + "." + key : key;
    if (value && typeof value === "object" && !Array.isArray(value)) out.push(...flatten(value, full));
    else out.push(full);
  }
  return out;
}

function checkI18n() {
  const enPath = "src/i18n/locales/en-US.json";
  const zhPath = "src/i18n/locales/zh-CN.json";
  const en = JSON.parse(read(enPath));
  const zh = JSON.parse(read(zhPath));
  const enSet = new Set(flatten(en));
  const zhSet = new Set(flatten(zh));
  const onlyEn = [...enSet].filter((key) => !zhSet.has(key)).sort();
  const onlyZh = [...zhSet].filter((key) => !enSet.has(key)).sort();

  if (onlyEn.length || onlyZh.length) {
    fail(
      "i18n parity",
      "i18n key mismatch. only en-US: " +
        onlyEn.join(", ") +
        "; only zh-CN: " +
        onlyZh.join(", "),
    );
    return;
  }

  const securitySource = read("src/components/Settings/panels/SecuritySettings.tsx");
  const referenced = [
    ...securitySource.matchAll(/\bt\(\s*["'](settings\.security[^"']+)["']\s*[,)]/g),
  ].map((match) => match[1]);
  const missing = [...new Set(referenced)].filter((key) => !enSet.has(key) || !zhSet.has(key));
  if (missing.length) {
    fail("i18n parity", "Security settings reference missing i18n keys: " + missing.join(", "));
  } else {
    pass("i18n parity");
  }
}

function checkPackageContract() {
  const pkg = JSON.parse(read("package.json"));
  if (pkg.scripts?.["docs:check"] !== "node scripts/check-docs.js") {
    fail("package docs command", 'package.json must define "docs:check": "node scripts/check-docs.js".');
  } else {
    pass("package docs command");
  }
}

function main() {
  checkRequiredFiles();
  checkLegacyFiles();
  checkReadmes();
  checkLinks();
  checkOwnerLanguage();
  checkChangelog();
  checkI18n();
  checkPackageContract();

  const passed = checks.filter((check) => check.ok).length;
  console.log("docs:check " + passed + "/" + checks.length + " checks passed");

  for (const item of checks) {
    console.log("  " + (item.ok ? "✓" : "✗") + " " + item.name);
  }
  for (const item of warnings) console.log("  ⚠ " + item);

  if (errors.length) {
    console.error("\nDocumentation contract errors:");
    for (const item of errors) console.error("  - " + item);
    process.exit(1);
  }

  console.log("Documentation contract is consistent.");
}

main();
