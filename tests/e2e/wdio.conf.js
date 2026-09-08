import path from "node:path";
import { fileURLToPath } from "node:url";

const here = path.dirname(fileURLToPath(import.meta.url));
const defaultBinary = process.platform === "win32"
  ? path.resolve(here, "../../src-tauri/target/debug/tauterm.exe")
  : path.resolve(here, "../../src-tauri/target/debug/tauterm");
const application = process.env.TAUTERM_E2E_BINARY || defaultBinary;

export const config = {
  runner: "local",
  specs: ["./specs/**/*.spec.js"],
  maxInstances: 1,
  logLevel: "warn",
  services: [
    [
      "@wdio/tauri-service",
      {
        driverProvider: "external",
        autoInstallTauriDriver: true,
      },
    ],
  ],
  capabilities: [
    {
      browserName: "tauri",
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
};
