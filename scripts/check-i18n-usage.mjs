#!/usr/bin/env node
/**
 * Core UI i18n usage contract.
 *
 * Every static `t("...")` key used by core UI code must exist in both shipped
 * locales. Plugin namespaces are owned by their plugin locale bundles and are
 * skipped here; runtime-merged namespaces (updaterError) are skipped as well.
 *
 * This guards the class of regression where a new UI action renders a raw key or
 * an English defaultValue in one language only (for example a "Saving..." button).
 */
import { readFileSync, readdirSync } from "node:fs";
import { execSync } from "node:child_process";
import path from "node:path";

const ROOT = process.cwd();
const LANGUAGES = ["en-US", "zh-CN"];
const EXTERNAL_NAMESPACES = new Set(["updaterError"]);

function read(relativePath) {
  return readFileSync(path.join(ROOT, relativePath), "utf8");
}

function flattenKeys(value, prefix = "", keys = new Set()) {
  for (const [key, child] of Object.entries(value)) {
    const fullKey = prefix ? `${prefix}.${key}` : key;
    if (child && typeof child === "object") {
      flattenKeys(child, fullKey, keys);
    } else {
      keys.add(fullKey);
    }
  }
  return keys;
}

const localeKeys = Object.fromEntries(
  LANGUAGES.map(language => [
    language,
    flattenKeys(JSON.parse(read(`src/i18n/locales/${language}.json`))),
  ]),
);

const pluginIds = new Set(
  readdirSync(path.join(ROOT, "src/plugin-manifests"))
    .filter(file => file.endsWith(".json"))
    .map(file => JSON.parse(read(`src/plugin-manifests/${file}`)).id),
);

const staticKeyPattern = /(?:\bt|i18n\.t)\(\s*(?:"([A-Za-z0-9_.-]+)"|'([A-Za-z0-9_.-]+)')/g;
const sourceFiles = execSync("git ls-files src", { encoding: "utf8", cwd: ROOT })
  .split("\n")
  .filter(file => /\.(ts|tsx)$/.test(file) && !file.startsWith("src/i18n/"));

const missing = [];
for (const file of sourceFiles) {
  const lines = read(file).split("\n");
  lines.forEach((line, index) => {
    for (const match of line.matchAll(staticKeyPattern)) {
      const key = match[1] ?? match[2];
      // `t("prefix." + dynamicKey)` concatenations are runtime-composed; the
      // prefix itself is a namespace, not a leaf key.
      if (key.endsWith(".")) continue;
      const namespace = key.split(".")[0];
      if (pluginIds.has(namespace) || EXTERNAL_NAMESPACES.has(namespace)) continue;
      const missingLanguages = LANGUAGES.filter(language => !localeKeys[language].has(key));
      if (missingLanguages.length > 0) {
        missing.push({ file, line: index + 1, key, missingLanguages });
      }
    }
  });
}

if (missing.length > 0) {
  console.error("i18n-usage: core UI i18n keys missing from shipped locales:");
  for (const entry of missing) {
    console.error(
      `  ${entry.file}:${entry.line}  ${entry.key}  (missing: ${entry.missingLanguages.join(", ")})`,
    );
  }
  process.exitCode = 1;
} else {
  console.log(
    `i18n-usage: ${sourceFiles.length} core source files use only keys present in ${LANGUAGES.join(" / ")}`,
  );
}
