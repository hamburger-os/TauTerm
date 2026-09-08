import fs from "node:fs";

function fail(message) {
  console.error("Runtime error/i18n contract failed: " + message);
  process.exitCode = 1;
}

const boundary = fs.readFileSync("src/components/common/ErrorBoundary.tsx", "utf8");
if (/console\.(?:error|warn)\s*\(/.test(boundary)) {
  fail("ErrorBoundary must route failures through the shared diagnostic bridge.");
}
if (/[\u3400-\u9fff]/.test(boundary.replace(/\/\*[\s\S]*?\*\//g, "").replace(/\/\/.*$/gm, ""))) {
  fail("ErrorBoundary contains hard-coded CJK user-facing text.");
}
for (const key of ["runtimeErrors.title", "runtimeErrors.unknown", "runtimeErrors.retry"]) {
  if (!boundary.includes(key)) fail("ErrorBoundary missing i18n key " + key);
}

const runtime = fs.readFileSync("src/utils/runtimeDiagnostics.ts", "utf8");
for (const required of ["window.error", "unhandledrejection", 'invoke("log_event"']) {
  if (!runtime.includes(required)) fail("runtimeDiagnostics missing " + required);
}

const locales = [
  JSON.parse(fs.readFileSync("src/i18n/locales/en-US.json", "utf8")),
  JSON.parse(fs.readFileSync("src/i18n/locales/zh-CN.json", "utf8")),
];
for (const locale of locales) {
  if (!locale.runtimeErrors?.title || !locale.runtimeErrors?.unknown || !locale.runtimeErrors?.retry) {
    fail("runtimeErrors locale contract is incomplete.");
  }
  if (!locale.diagnostics?.export || !locale.diagnostics?.exportFailed) {
    fail("diagnostics locale contract is incomplete.");
  }
}

if (process.exitCode) process.exit(process.exitCode);
console.log("Runtime error/i18n contract passed.");
