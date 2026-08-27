import { readFileSync } from "node:fs";
import { resolve } from "node:path";

const root = resolve(import.meta.dirname, "..");
const path = resolve(root, "rust-toolchain.toml");
const content = readFileSync(path, "utf8");

function fail(message) {
  console.error(`❌ rust-toolchain.toml: ${message}`);
  process.exit(1);
}

const channel = content.match(/^\s*channel\s*=\s*"([^"]+)"\s*$/m)?.[1];
if (!channel) {
  fail("missing [toolchain] channel");
}
if (!/^\d+\.\d+\.\d+$/.test(channel)) {
  fail(`channel must be an exact stable Rust version, found ${channel}`);
}

const profile = content.match(/^\s*profile\s*=\s*"([^"]+)"\s*$/m)?.[1];
if (profile !== "minimal") {
  fail(`profile must be "minimal", found ${profile ?? "missing"}`);
}

const componentsText = content.match(/^\s*components\s*=\s*\[([^\]]*)\]\s*$/m)?.[1];
if (!componentsText) {
  fail("missing components");
}

const components = [...componentsText.matchAll(/"([^"]+)"/g)].map((match) => match[1]);
for (const component of ["clippy", "rustfmt"]) {
  if (!components.includes(component)) {
    fail(`missing required component ${component}`);
  }
}

console.log(`✅ Rust toolchain is pinned to ${channel} with clippy + rustfmt.`);
