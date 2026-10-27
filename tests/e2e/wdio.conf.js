import os from "node:os";
import path from "node:path";
import { spawn } from "node:child_process";
import { fileURLToPath } from "node:url";
import { access, mkdir } from "node:fs/promises";
import { setTimeout as delay } from "node:timers/promises";

const here = path.dirname(fileURLToPath(import.meta.url));
const defaultBinary = process.platform === "win32"
  ? path.resolve(here, "../../.cache/e2e/app/tauterm.exe")
  : path.resolve(here, "../../.cache/e2e/app/tauterm");
const application = process.env.TAUTERM_E2E_BINARY || defaultBinary;
const cargoHome = process.env.CARGO_HOME || path.join(os.homedir(), ".cargo");
const tauriDriver = process.env.TAUTERM_E2E_DRIVER || path.join(
  cargoHome,
  "bin",
  process.platform === "win32" ? "tauri-driver.exe" : "tauri-driver",
);

let driverProcess;
let shuttingDown = false;
const port = Number(process.env.TAUTERM_E2E_PORT || 4444);

function stopDriver() {
  shuttingDown = true;
  if (driverProcess && !driverProcess.killed) {
    driverProcess.kill();
  }
  driverProcess = undefined;
}

export const config = {
  hostname: "127.0.0.1",
  port,
  runner: "local",
  specs: ["./specs/**/*.spec.js"],
  maxInstances: 1,
  logLevel: "warn",
  capabilities: [
    {
      maxInstances: 1,
      "tauri:options": {
        application,
      },
    },
  ],
  framework: "mocha",
  reporters: ["spec"],
  waitforTimeout: 15_000,
  connectionRetryTimeout: 120_000,
  connectionRetryCount: 2,
  mochaOpts: {
    ui: "bdd",
    timeout: 90_000,
  },
  beforeSession: async () => {
    await Promise.all([access(application), access(tauriDriver)]);
    shuttingDown = false;
    const args = ["--port", String(port), "--native-port", String(port + 1)];
    if (process.env.TAUTERM_E2E_NATIVE_DRIVER) args.push("--native-driver", process.env.TAUTERM_E2E_NATIVE_DRIVER);
    driverProcess = spawn(tauriDriver, args, {
      stdio: [null, process.stdout, process.stderr],
      env: process.env,
    });
    driverProcess.on("error", error => {
      console.error("tauri-driver error:", error);
      process.exitCode = 1;
    });
    driverProcess.on("exit", code => {
      if (!shuttingDown && code !== 0 && code !== null) {
        console.error("tauri-driver exited unexpectedly with code:", code);
        process.exitCode = 1;
      }
    });
    const deadline = Date.now() + 15_000;
    while (Date.now() < deadline) {
      if (driverProcess.exitCode !== null) throw new Error(`tauri-driver exited: ${driverProcess.exitCode}`);
      try {
        const status = await fetch(`http://127.0.0.1:${port}/status`, { signal: AbortSignal.timeout(2000) });
        if (status.ok) return;
      } catch { /* The external driver is still starting. */ }
      await delay(100);
    }
    stopDriver();
    throw new Error("tauri-driver did not become ready within 15 seconds");
  },
  afterTest: async (_test, _context, { passed }) => {
    if (passed) return;
    const identifier = await browser.executeAsync(async done => {
      try { done(await window.__TAURI_INTERNALS__.invoke("plugin:app|identifier")); }
      catch { done(null); }
    });
    if (identifier !== "com.tauterm.e2e") return;
    const artifacts = path.resolve(here, "../../.cache/e2e/artifacts");
    await mkdir(artifacts, { recursive: true });
    await browser.saveScreenshot(path.join(artifacts, `failure-${Date.now()}.png`));
    console.error("TauTerm E2E failure:", await browser.execute(() => document.body.innerText.slice(-8000)));
  },
  afterSession: () => {
    stopDriver();
  },
};

for (const signal of ["SIGINT", "SIGTERM", "SIGHUP"]) {
  process.on(signal, () => stopDriver());
}
process.on("exit", () => stopDriver());
