#!/usr/bin/env node
import { readFileSync } from "node:fs";
import { resolve } from "node:path";

const root = resolve(import.meta.dirname, "..");
const pkg = JSON.parse(readFileSync(resolve(root, "package.json"), "utf8"));
const version = (process.argv[2] ?? pkg.version).replace(/^v/, "");

if (!/^\d+\.\d+\.\d+(?:-(?:alpha|beta|rc)\.\d+)?$/.test(version)) {
  throw new Error("Unsupported release version: " + version);
}

const changelog = readFileSync(resolve(root, "CHANGELOG.md"), "utf8");
const lines = changelog.split(/\r?\n/);
const prefix = "## [" + version + "]";
const start = lines.findIndex((line) => line === prefix || line.startsWith(prefix + " "));

if (start < 0) {
  throw new Error("CHANGELOG.md has no release section for " + version);
}

let end = lines.length;
for (let index = start + 1; index < lines.length; index += 1) {
  if (/^##\s+\[/.test(lines[index])) {
    end = index;
    break;
  }
}

const body = lines.slice(start + 1, end).join("\n").trim();
if (body.length < 20) {
  throw new Error("CHANGELOG.md release section is empty for " + version);
}

process.stdout.write(body + "\n");
