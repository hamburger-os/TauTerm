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

const updaterHook = fs.readFileSync("src/hooks/useUpdater.ts", "utf8");
for (const required of [
  "classifyUpdaterError",
  "isRetryableUpdaterError",
  "reportFrontendError",
  "MANUAL_CHECK_TIMEOUT_MS = 30_000",
  'import.meta.env.PROD && import.meta.env.TAURI_ENV_DEBUG === "false"',
  "if (!UPDATER_RUNTIME_ENABLED)",
  "if (!UPDATER_RUNTIME_ENABLED || !shouldAutoCheck()) return",
  'recordFailure("download-install"',
  'recordFailure("relaunch"',
]) {
  if (!updaterHook.includes(required)) fail("useUpdater missing " + required);
}
if (/error:\s*String\s*\(/.test(updaterHook)) {
  fail("useUpdater must not expose raw updater errors directly in the UI.");
}

const viteConfig = fs.readFileSync("vite.config.ts", "utf8");
if (!viteConfig.includes('envPrefix: ["VITE_", "TAURI_ENV_"]')) {
  fail("Vite must expose TAURI_ENV_* so release/debug updater policy reaches the frontend bundle.");
}

const releaseAssembler = fs.readFileSync("scripts/assemble-release.js", "utf8");
if (!releaseAssembler.includes('["windows-x86_64-nsis", "_x64-setup.exe"]')) {
  fail("release manifest must publish the exact Windows NSIS updater target.");
}
if (releaseAssembler.includes('["windows-x86_64",')) {
  fail("release manifest must not publish a generic Windows compatibility target.");
}

const xtermLifecycle = fs.readFileSync("src/components/Terminal/xtermLifecycle.ts", "utf8");
for (const required of [
  "host.isConnected",
  "host.clientWidth > 0",
  "host.clientHeight > 0",
  "const scheduleInitialize = () =>",
  "bootstrapObserver = new ResizeObserver(scheduleInitialize)",
  "resizeObserver = new ResizeObserver(scheduleFit)",
  "cancelAnimationFrame(fitRaf)",
  "openCleanup?.()",
  "bundle?.terminal.dispose()",
]) {
  if (!xtermLifecycle.includes(required)) fail("Shared xterm lifecycle contract missing " + required);
}
const lifecycleFitCalls = xtermLifecycle.match(/\.fitAddon\.fit\(\)/g) ?? [];
if (lifecycleFitCalls.length !== 1) {
  fail("Shared xterm lifecycle must own the single guarded FitAddon.fit path.");
}

const terminal = fs.readFileSync("src/components/Terminal/Terminal.tsx", "utf8");
for (const required of [
  "createManagedXTermHost",
  "xtermLayoutRef.current?.scheduleFit()",
  "onFit: notifyResize",
  "if (opened) {",
  "onCleanupRef.current?.(sessionId)",
]) {
  if (!terminal.includes(required)) fail("Terminal lifecycle integration missing " + required);
}
if (/\.open\s*\(|\.fit\s*\(/.test(terminal)) {
  fail("Terminal must not bypass the shared xterm lifecycle with direct open/fit calls.");
}

const rttTerminal = fs.readFileSync("src/plugins/rtt/RttTerminalView.tsx", "utf8");
if (!rttTerminal.includes("createManagedXTermHost")) {
  fail("RTT Terminal must reuse the shared xterm lifecycle.");
}
if (/\.open\s*\(|\.fit\s*\(/.test(rttTerminal)) {
  fail("RTT Terminal must not bypass the shared xterm lifecycle with direct open/fit calls.");
}

const windowsDriver = fs.readFileSync("src-tauri/src/virtual_port/windows_driver.rs", "utf8");
for (const required of [
  "OpenSCManagerW",
  "OpenServiceW",
  "SC_MANAGER_CONNECT",
  "SERVICE_QUERY_STATUS",
  "CloseServiceHandle",
]) {
  if (!windowsDriver.includes(required)) fail("Windows driver SCM probe missing " + required);
}
if (/\bCommand::new\b|std::process::Command|CommandExt/.test(windowsDriver)) {
  fail("Windows ordinary driver probe must use SCM API directly, not spawn an external command.");
}
if (/setupc/i.test(windowsDriver.replace(/\/\/!.*$/gm, ""))) {
  fail("Windows ordinary driver probe must not execute setupc.");
}
const platformCommands = fs.readFileSync("src-tauri/src/commands/platform.rs", "utf8");
if (!platformCommands.includes("windows_driver::is_com0com_driver_installed")) {
  fail("virtual-port status/install commands must use the SCM-only Windows driver probe.");
}
const rustRuntime = fs.readFileSync("src-tauri/src/lib.rs", "utf8");
if (!rustRuntime.includes("windows_driver::is_com0com_driver_installed")) {
  fail("Windows startup must use the SCM-only driver probe.");
}
const directFallback = rustRuntime.indexOf("direct-uac-on-demand");
const linuxBranch = rustRuntime.indexOf('#[cfg(any(target_os = "linux", target_os = "macos"))]');
if (directFallback < 0 || linuxBranch < 0) {
  fail("Windows direct UAC fallback boundary is missing.");
} else {
  const windowsStartup = rustRuntime.slice(directFallback, linuxBranch);
  if (windowsStartup.includes("cleanup_orphans()")) {
    fail("Windows direct UAC fallback must not perform startup orphan cleanup.");
  }
}

const updaterClassifier = fs.readFileSync("src/utils/updaterError.ts", "utf8");
for (const kind of [
  "timeout",
  "dns",
  "proxy",
  "tls",
  "http",
  "metadata",
  "signature",
  "transport",
  "install",
  "relaunch",
  "unknown",
]) {
  if (!updaterClassifier.includes(`| "${kind}"`) && !updaterClassifier.includes(`kind: "${kind}"`)) {
    fail("updater error classifier missing category " + kind);
  }
}
if (!updaterClassifier.includes('return kind === "timeout" || kind === "transport"')) {
  fail("updater retry policy must stay limited to transient timeout/transport failures.");
}

const updaterMessages = fs.readFileSync("src/i18n/updaterErrors.ts", "utf8");
for (const locale of ['"zh-CN"', '"en-US"']) {
  if (!updaterMessages.includes(locale)) fail("updater error messages missing locale " + locale);
}
for (const key of ["timeout", "dns", "proxy", "tls", "http", "metadata", "signature", "transport", "install", "relaunch", "unknown"]) {
  const occurrences = updaterMessages.match(new RegExp(`\\b${key}:`, "g"))?.length ?? 0;
  if (occurrences < 2) fail("updater error messages incomplete for " + key);
}

const i18nIndex = fs.readFileSync("src/i18n/index.ts", "utf8");
if (!i18nIndex.includes("updaterErrorTranslations")) {
  fail("i18n resources must register updater error translations.");
}

const aboutSettings = fs.readFileSync("src/components/Settings/panels/AboutSettings.tsx", "utf8");
if (!aboutSettings.includes('t("updaterError.unknown")')) {
  fail("AboutSettings must provide the localized updater error fallback.");
}
if (aboutSettings.includes('t("updater.checkFailed"')) {
  fail("AboutSettings must not wrap every updater stage as a check failure.");
}

// Rust and JS updater bindings are a release-infrastructure pair. Keep them on
// the same exact resolved version so an npm/cargo refresh cannot silently drift.
const cargoToml = fs.readFileSync("src-tauri/Cargo.toml", "utf8");
const rustSpec = cargoToml.match(/tauri-plugin-updater\s*=\s*"=(\d+\.\d+\.\d+)"/);
if (!rustSpec) {
  fail("tauri-plugin-updater must be pinned to an exact version in Cargo.toml.");
} else {
  const cargoLock = fs.readFileSync("src-tauri/Cargo.lock", "utf8");
  const rustResolved = cargoLock.match(/\[\[package\]\]\s+name = "tauri-plugin-updater"\s+version = "([^"]+)"/);
  const packageLock = JSON.parse(fs.readFileSync("package-lock.json", "utf8"));
  const jsResolved = packageLock.packages?.["node_modules/@tauri-apps/plugin-updater"]?.version;
  if (!rustResolved) fail("Cargo.lock is missing tauri-plugin-updater.");
  if (!jsResolved) fail("package-lock.json is missing @tauri-apps/plugin-updater.");
  if (rustResolved && rustResolved[1] !== rustSpec[1]) {
    fail(`Cargo.toml updater ${rustSpec[1]} does not match Cargo.lock ${rustResolved[1]}.`);
  }
  if (rustResolved && jsResolved && rustResolved[1] !== jsResolved) {
    fail(`Rust updater ${rustResolved[1]} does not match JS updater ${jsResolved}.`);
  }
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
