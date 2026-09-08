import os from "node:os";
import path from "node:path";
import { spawn } from "node:child_process";
import { fileURLToPath } from "node:url";

const here = path.dirname(fileURLToPath(import.meta.url));
const defaultBinary = process.platform === "win32"
  ? path.resolve(here, "../../src-tauri/target/debug/tauterm.exe")
  : path.resolve(here, "../../src-tauri/target/debug/tauterm");
const application = process.env.TAUTERM_E2E_BINARY || defaultBinary;
const cargoHome = process.env.CARGO_HOME || path.join(os.homedir(), ".cargo");
const tauriDriver = path.join(
  cargoHome,
  "bin",
  process.platform === "win32" ? "tauri-driver.exe" : "tauri-driver",
);

let driverProcess;
let shuttingDown = false;

function stopDriver() {
  shuttingDown = true;
  if (driverProcess && !driverProcess.killed) {
    driverProcess.kill();
  }
  driverProcess = undefined;
}

export const config = {
  host: "127.0.0.1",
  port: 4444,
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
  beforeSession: () => {
    shuttingDown = false;
    driverProcess = spawn(tauriDriver, [], {
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
  },
  afterSession: () => {
    stopDriver();
  },
};

for (const signal of ["SIGINT", "SIGTERM", "SIGHUP"]) {
  process.on(signal, () => stopDriver());
}
process.on("exit", () => stopDriver());
