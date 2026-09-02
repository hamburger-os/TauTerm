#!/usr/bin/env node

/**
 * Builds the exact generation prompt and mandatory style-reference list for
 * one TauTerm icon. Semantic content stays in prompts.md; machine-readable
 * family anchors and palette limits stay in style-contract.json.
 */
import { existsSync, readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const ICON_DIR = resolve(ROOT, "src/assets/icons");
const promptSource = readFileSync(resolve(ICON_DIR, "prompts.md"), "utf8");
const contract = JSON.parse(readFileSync(resolve(ICON_DIR, "style-contract.json"), "utf8"));
const rows = new Map(
  [...promptSource.matchAll(/^\|\s*`([^`]+)`\s*\|\s*([^|\n]+?)\s*\|\s*([^|\n]+?)\s*\|\s*$/gm)]
    .map((match) => [match[1], { role: match[2].trim(), shape: match[3].trim() }]),
);

function fencedValue(label) {
  const escaped = label.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  const match = promptSource.match(new RegExp(`\\*\\*${escaped}：\\*\\*\\s*\\n\\s*\`([^\`]+)\``));
  if (!match) throw new Error(`prompts.md is missing ${label}`);
  return match[1].trim();
}

const key = process.argv.slice(2).find((arg) => !arg.startsWith("-"));
const asJson = process.argv.includes("--json");
if (!key) {
  console.error("Usage: npm run prompt:icon -- <icon-key> [--json]");
  process.exit(2);
}
const semantic = rows.get(key);
if (!semantic) {
  console.error(`Unknown icon key: ${key}. Add its semantic row to src/assets/icons/prompts.md first.`);
  process.exit(2);
}
if (contract.brandExceptions.includes(key)) {
  console.error(`${key} is a brand exception and must not use the functional-icon generator.`);
  process.exit(2);
}

const family = contract.microControlKeys.includes(key) ? "micro-control" : "functional";
const referenceNames = family === "micro-control"
  ? contract.microControlReferences
  : contract.functionalReferences;
const references = referenceNames.map((name) => resolve(ICON_DIR, `${name}.png`));
for (const reference of references) {
  if (!existsSync(reference)) throw new Error(`Missing mandatory style reference: ${reference}`);
}

const base = fencedValue("基础正向提示词").replace("[SEMANTIC SHAPE]", semantic.shape);
const negative = fencedValue("负向提示词");
const prompt = [
  "Use case: precise-object-edit.",
  "Asset type: TauTerm 256x256 transparent PNG functional UI icon.",
  `Semantic role: ${semantic.role}.`,
  `Primary request: ${base}`,
  `Reference contract: the ${references.length} attached images are mandatory strict style anchors for the ${family} family. Inherit their pale ice-blue palette, rounded inflated geometry, glass thickness, highlight softness, optical weight and transparent padding; do not copy their semantic shapes.`,
  `Semantic constraints: ${semantic.shape}`,
  "Composition: one centered glyph, no extra objects, readable at 12px, inside the same 192px optical frame as the references.",
  `Avoid: ${negative}`,
  "Do not reinterpret, omit or override the reference contract even if the semantic subject commonly uses metallic, purple, dark or multicolour styling.",
].join("\n");

if (asJson) {
  console.log(JSON.stringify({ key, family, references, prompt }, null, 2));
} else {
  console.log(`Icon: ${key}`);
  console.log(`Family: ${family}`);
  console.log("Attach these reference images in this order:");
  references.forEach((reference, index) => console.log(`  ${index + 1}. ${reference}`));
  console.log("\nUse this prompt verbatim:\n");
  console.log(prompt);
}
