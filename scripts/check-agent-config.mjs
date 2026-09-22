#!/usr/bin/env node
/**
 * Validate repository AI-agent configuration.
 *
 * The repository follows the open AGENTS.md and Agent Skills conventions.
 * This checker intentionally validates only portable, mechanical constraints
 * plus TauTerm's local hygiene rules.
 */
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.resolve(
  process.argv.includes("--root")
    ? process.argv[process.argv.indexOf("--root") + 1]
    : path.join(path.dirname(fileURLToPath(import.meta.url)), ".."),
);

const errors = [];
const warnings = [];

function fail(message) {
  errors.push(message);
}

function warn(message) {
  warnings.push(message);
}

function read(rel) {
  return fs.readFileSync(path.join(root, rel), "utf8");
}

function walk(relDir) {
  const abs = path.join(root, relDir);
  if (!fs.existsSync(abs)) return [];
  const out = [];
  for (const entry of fs.readdirSync(abs, { withFileTypes: true })) {
    const rel = path.posix.join(relDir.replaceAll("\\", "/"), entry.name);
    if (entry.isDirectory()) out.push(...walk(rel));
    else out.push(rel);
  }
  return out;
}

function unquote(raw, rel, key) {
  const value = raw.trim();
  if (!value) return "";
  if (value.startsWith('"')) {
    try {
      return JSON.parse(value);
    } catch {
      fail(`${rel}: invalid double-quoted YAML scalar for ${key}`);
      return value;
    }
  }
  if (value.startsWith("'")) {
    if (!value.endsWith("'") || value.length < 2) {
      fail(`${rel}: invalid single-quoted YAML scalar for ${key}`);
      return value;
    }
    return value.slice(1, -1).replaceAll("''", "'");
  }
  return value;
}

function parseFrontmatter(rel, content) {
  const lines = content.split(/\r?\n/);
  if (lines[0]?.trim() !== "---") {
    fail(`${rel}: SKILL.md must start with YAML frontmatter`);
    return { fields: new Map(), lines };
  }

  const end = lines.findIndex((line, index) => index > 0 && line.trim() === "---");
  if (end < 0) {
    fail(`${rel}: YAML frontmatter is not closed`);
    return { fields: new Map(), lines };
  }

  const fields = new Map();
  const metadataEntries = new Map();
  let activeTopLevel = null;
  for (let index = 1; index < end; index += 1) {
    const line = lines[index];
    if (!line.trim() || line.trimStart().startsWith("#")) continue;

    const indent = line.length - line.trimStart().length;
    if (indent > 0) {
      if (activeTopLevel !== "metadata") {
        fail(`${rel}:${index + 1}: nested YAML is only allowed under metadata`);
        continue;
      }

      const metadataMatch = line.trim().match(/^([^:]+):(?:\s*(.*))?$/);
      if (!metadataMatch) {
        fail(`${rel}:${index + 1}: metadata must be a flat string-to-string mapping`);
        continue;
      }

      const [, rawKey, rawValue = ""] = metadataMatch;
      const key = rawKey.trim();
      if (!key) {
        fail(`${rel}:${index + 1}: metadata key must be a non-empty string`);
        continue;
      }
      if (metadataEntries.has(key)) {
        fail(`${rel}:${index + 1}: duplicate metadata key "${key}"`);
        continue;
      }

      const value = unquote(rawValue, rel, `metadata.${key}`);
      if (!value) {
        fail(`${rel}:${index + 1}: metadata value for "${key}" must be a non-empty string`);
      }
      if (/^(?:null|~|true|false|[-+]?\d+(?:\.\d+)?)$/i.test(rawValue.trim())) {
        fail(`${rel}:${index + 1}: metadata value for "${key}" must be a YAML string, not a boolean/number/null scalar`);
      }
      metadataEntries.set(key, value);
      continue;
    }

    const match = line.match(/^([A-Za-z0-9_-]+):(?:\s*(.*))?$/);
    if (!match) {
      fail(`${rel}:${index + 1}: unsupported top-level frontmatter syntax`);
      continue;
    }

    const [, key, rawValue = ""] = match;
    if (fields.has(key)) fail(`${rel}: duplicate frontmatter field "${key}"`);
    fields.set(key, rawValue);
    activeTopLevel = key;
  }

  if (fields.has("metadata") && fields.get("metadata").trim()) {
    fail(`${rel}: metadata must be a mapping, not a scalar value`);
  }

  return { fields, metadataEntries, lines };
}

const agentsPath = "AGENTS.md";
if (!fs.existsSync(path.join(root, agentsPath))) {
  fail("Missing root AGENTS.md");
} else {
  const agentsBytes = Buffer.byteLength(read(agentsPath), "utf8");
  if (agentsBytes > 24 * 1024) {
    warn(`AGENTS.md is ${agentsBytes} bytes; keep repository-wide instructions concise so nested instructions still have context budget.`);
  }
}

const skillsRoot = ".agents/skills";
const skillsAbs = path.join(root, skillsRoot);
if (!fs.existsSync(skillsAbs)) {
  fail("Missing .agents/skills directory");
}

const allowedFields = new Set([
  "name",
  "description",
  "license",
  "compatibility",
  "metadata",
  "allowed-tools",
]);

const skillDirs = fs.existsSync(skillsAbs)
  ? fs.readdirSync(skillsAbs, { withFileTypes: true }).filter((entry) => entry.isDirectory())
  : [];

const seenNames = new Map();
let validated = 0;

for (const entry of skillDirs) {
  const dirName = entry.name;
  const rel = path.posix.join(skillsRoot, dirName, "SKILL.md");
  const abs = path.join(root, rel);
  if (!fs.existsSync(abs)) {
    fail(`${skillsRoot}/${dirName}: missing SKILL.md`);
    continue;
  }

  const content = fs.readFileSync(abs, "utf8");
  const { fields, lines } = parseFrontmatter(rel, content);
  const fieldNames = [...fields.keys()];

  for (const key of fieldNames) {
    if (!allowedFields.has(key)) fail(`${rel}: unsupported frontmatter field "${key}"`);
  }

  if (!fields.has("name")) fail(`${rel}: missing required frontmatter field "name"`);
  if (!fields.has("description")) fail(`${rel}: missing required frontmatter field "description"`);

  const name = unquote(fields.get("name") ?? "", rel, "name");
  const description = unquote(fields.get("description") ?? "", rel, "description");
  const compatibility = fields.has("compatibility")
    ? unquote(fields.get("compatibility") ?? "", rel, "compatibility")
    : null;
  const allowedTools = fields.has("allowed-tools")
    ? unquote(fields.get("allowed-tools") ?? "", rel, "allowed-tools")
    : null;

  if (!/^[a-z0-9]+(?:-[a-z0-9]+)*$/.test(name) || name.length > 64) {
    fail(`${rel}: name must be 1-64 chars of lowercase letters, digits, and single hyphens`);
  }
  if (name !== dirName) {
    fail(`${rel}: name "${name}" must match parent directory "${dirName}"`);
  }
  if (description.length < 1 || description.length > 1024) {
    fail(`${rel}: description must be 1-1024 characters`);
  }
  if (compatibility !== null && (compatibility.length < 1 || compatibility.length > 500)) {
    fail(`${rel}: compatibility must be 1-500 characters when provided`);
  }
  if (allowedTools !== null && !allowedTools.trim()) {
    fail(`${rel}: allowed-tools must be a non-empty space-separated string when provided`);
  }

  if (seenNames.has(name)) fail(`${rel}: duplicate skill name also used by ${seenNames.get(name)}`);
  else seenNames.set(name, rel);

  if (lines.length >= 500) {
    fail(`${rel}: ${lines.length} lines is not under TauTerm's 500-line SKILL.md limit; move detail into references/`);
  }

  const skillRoot = path.posix.join(skillsRoot, dirName);
  for (const resourceDir of ["references", "scripts", "assets"]) {
    const resourceRoot = path.posix.join(skillRoot, resourceDir);
    for (const resource of walk(resourceRoot)) {
      const local = resource.slice(skillRoot.length + 1);
      const depth = local.split("/").length;
      if (depth > 2) {
        fail(`${resource}: keep skill resources one level deep from SKILL.md`);
      }
      if (resourceDir === "references" && !content.includes(local)) {
        fail(`${resource}: reference file is orphaned; link or mention "${local}" from SKILL.md`);
      }
    }
  }

  for (const match of content.matchAll(/\((?:(?:references|scripts|assets)\/[^)#?]+)(?:#[^)]*)?\)/g)) {
    const target = match[0].slice(1, -1).split("#")[0];
    if (!fs.existsSync(path.join(root, skillRoot, target))) {
      fail(`${rel}: missing skill resource "${target}"`);
    }
  }

  validated += 1;
}

if (validated === 0) fail("No Agent Skills were found under .agents/skills");

console.log(`agents:check validated ${validated} skill(s)`);
for (const message of warnings) console.log(`  ⚠ ${message}`);

if (errors.length) {
  console.error("\nAgent configuration errors:");
  for (const message of errors) console.error(`  - ${message}`);
  process.exit(1);
}

console.log("Agent configuration is consistent.");
