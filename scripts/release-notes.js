#!/usr/bin/env node
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { extractReleaseNotes, normalizeReleaseVersion } from "./release-changelog.mjs";

const root = resolve(import.meta.dirname, "..");
const pkg = JSON.parse(readFileSync(resolve(root, "package.json"), "utf8"));
const version = normalizeReleaseVersion(process.argv[2] ?? pkg.version);
const changelog = readFileSync(resolve(root, "CHANGELOG.md"), "utf8");

process.stdout.write(extractReleaseNotes(changelog, version) + "\n");
