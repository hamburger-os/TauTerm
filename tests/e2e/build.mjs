import { spawnSync } from "node:child_process";
import { createRequire } from "node:module";
import { mkdir, copyFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";
import path from "node:path";

const root = fileURLToPath(new URL("../../", import.meta.url));
const require = createRequire(import.meta.url);
const cli = require.resolve("@tauri-apps/cli/tauri.js");
const result = spawnSync(process.execPath, [cli, "build", "--debug", "--no-bundle", "--config", "tests/e2e/tauri.conf.json"], {
  cwd: root, stdio: "inherit",
});
if (result.error) throw result.error;
if (result.status !== 0) process.exit(result.status ?? 1);
const binary = process.platform === "win32" ? "tauterm.exe" : "tauterm";
const output = path.join(root, ".cache/e2e/app");
await mkdir(output, { recursive: true });
await copyFile(path.join(root, "src-tauri/target/debug", binary), path.join(output, binary));
console.log(`Isolated E2E application: ${path.join(output, binary)}`);
