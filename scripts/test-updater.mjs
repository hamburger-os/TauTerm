import assert from "node:assert/strict";
import { test } from "node:test";
import { readFileSync } from "node:fs";
import { UpdaterController } from "../src/core/updater-controller.ts";
import { classifyUpdaterError } from "../src/utils/updaterError.ts";

const flush = () => new Promise(setImmediate);
const deferred = () => {
  let resolve, reject;
  const promise = new Promise((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
};

function resource(version = "9.0.0") {
  return {
    version, body: "Release notes", closed: 0, downloads: 0, installs: 0,
    async download(onEvent) {
      this.downloads++;
      onEvent({ event: "Started", data: {} });
      onEvent({ event: "Progress", data: { chunkLength: 40 } });
      onEvent({ event: "Finished" });
    },
    async install() { this.installs++; },
    async close() { this.closed++; },
  };
}

function fixture(mode = "installed", overrides = {}) {
  const state = { calls: [], reports: [], jobs: new Set(), frequency: "daily", touched: 0, restarts: 0 };
  const controller = new UpdaterController({
    loadPolicy: async () => ({ mode, target: mode === "unavailable" ? null : "windows-x86_64-nsis" }),
    check: async options => { state.calls.push(options); return null; },
    relaunch: async () => { state.restarts++; },
    report: (...args) => state.reports.push(args),
    getFrequency: () => state.frequency,
    setFrequency: value => { state.frequency = value; },
    shouldAutoCheck: () => state.frequency !== "never",
    touchLastCheck: () => { state.touched++; },
    schedule: callback => { state.jobs.add(callback); return () => state.jobs.delete(callback); },
    delay: async () => {},
    ...overrides,
  });
  return { controller, state, auto: () => {
    for (const job of state.jobs) { state.jobs.delete(job); job(); }
  } };
}

test("development checks manually using the exact target and releases query resources", async () => {
  const update = resource();
  const calls = [];
  const { controller, state } = fixture("queryOnly", { check: async options => { calls.push(options); return update; } });
  await controller.start();
  assert.equal(state.jobs.size, 0);
  assert.equal(calls.length, 0);
  await controller.check();
  assert.deepEqual(calls, [{ target: "windows-x86_64-nsis", timeout: 30000 }]);
  assert.deepEqual(controller.getSnapshot().info, { phase: "available", release: { version: update.version, notes: update.body } });
  await controller.download();
  await controller.install();
  await controller.relaunch();
  controller.setFrequency("always");
  controller.stop();
  assert.equal(update.closed, 1);
  assert.equal(update.downloads + update.installs + state.restarts, 0);
  assert.equal(state.frequency, "daily");
});

test("manual latest and unsupported platform have explicit terminal states", async () => {
  const { controller } = fixture("queryOnly");
  await controller.start();
  await controller.check();
  assert.equal(controller.getSnapshot().info.phase, "latest");
  controller.stop();
  const unsupported = fixture("unavailable");
  await unsupported.controller.start();
  await unsupported.controller.check();
  assert.equal(unsupported.controller.getSnapshot().info.phase, "unavailable");
  assert.equal(unsupported.state.calls.length, 0);
  unsupported.controller.stop();
});

test("manual joins an automatic check and upgrades its feedback without another request", async () => {
  const gate = deferred();
  let calls = 0;
  const { controller, auto } = fixture("installed", { check: () => { calls++; return gate.promise; } });
  await controller.start();
  auto();
  await flush();
  const joined = controller.check();
  assert.equal(controller.check(), joined);
  assert.equal(calls, 1);
  gate.resolve(null);
  await joined;
  assert.equal(controller.getSnapshot().info.phase, "latest");
  controller.stop();
});

test("never cancels a scheduled automatic check; manual check supersedes startup scheduling", async () => {
  const { controller, state, auto } = fixture();
  await controller.start();
  assert.equal(state.jobs.size, 1);
  controller.setFrequency("never");
  assert.equal(state.jobs.size, 0);
  auto();
  await flush();
  assert.equal(state.calls.length, 0);
  controller.setFrequency("always");
  assert.equal(state.jobs.size, 1);
  await controller.check();
  auto();
  await flush();
  assert.equal(state.calls.length, 1);
  controller.stop();
});

test("replacement and unmount close each update exactly once", async () => {
  const first = resource("1.0.0"), second = resource("2.0.0");
  let calls = 0;
  const { controller } = fixture("installed", { check: async () => ++calls === 1 ? first : second });
  await controller.start();
  await controller.check();
  await controller.check();
  assert.equal(first.closed, 1);
  controller.stop();
  controller.stop();
  await flush();
  assert.equal(first.closed, 1);
  assert.equal(second.closed, 1);
});

test("StrictMode restart rejects stale policy and check results and closes orphan resources", async () => {
  const gate = deferred(), orphan = resource();
  let calls = 0;
  const { controller } = fixture("installed", { check: () => ++calls === 1 ? gate.promise : Promise.resolve(null) });
  const starting = controller.start();
  controller.stop();
  await controller.start();
  await starting;
  const stale = controller.check();
  await flush();
  controller.stop();
  await controller.start();
  await controller.check();
  gate.resolve(orphan);
  await stale;
  assert.equal(controller.getSnapshot().info.phase, "latest");
  assert.equal(orphan.closed, 1);
  controller.stop();
});

test("download completes without installing; install and restart are separate guarded actions", async () => {
  const update = resource();
  const { controller, state } = fixture("installed", { check: async () => update });
  const snapshots = [];
  controller.subscribe(() => snapshots.push(controller.getSnapshot().info));
  await controller.start();
  await controller.check();
  await controller.install();
  assert.equal(update.installs, 0);
  const download = controller.download();
  await controller.download();
  await controller.check();
  await download;
  assert.equal(update.downloads, 1);
  assert.equal(update.installs, 0);
  assert.equal(controller.getSnapshot().info.phase, "downloaded");
  const unknownProgress = snapshots.find(info => info.phase === "downloading" && info.downloadedBytes === 40);
  assert.equal(unknownProgress.totalBytes, null);
  await controller.install();
  assert.equal(update.installs, 1);
  assert.equal(update.closed, 1);
  assert.equal(controller.getSnapshot().info.phase, "restartRequired");
  await controller.relaunch();
  assert.equal(state.restarts, 1);
  controller.stop();
});

test("installation stays busy until resource cleanup finishes so the restart button cannot silently fail", async () => {
  const gate = deferred(), update = resource();
  update.close = async () => { update.closed++; await gate.promise; };
  const { controller, state } = fixture("installed", { check: async () => update });
  await controller.start(); await controller.check(); await controller.download();
  const installing = controller.install();
  await flush();
  assert.equal(controller.getSnapshot().info.phase, "installing");
  gate.resolve(); await installing;
  assert.equal(controller.getSnapshot().info.phase, "restartRequired");
  await controller.relaunch();
  assert.equal(state.restarts, 1);
  controller.stop();
});

test("in-flight download is not closed until native completion after unmount", async () => {
  const gate = deferred(), update = resource();
  update.download = async () => { await gate.promise; };
  const { controller } = fixture("installed", { check: async () => update });
  await controller.start(); await controller.check();
  const downloading = controller.download();
  controller.stop();
  assert.equal(update.closed, 0);
  gate.resolve();
  await downloading; await flush();
  assert.equal(update.closed, 1);
});

test("retries preserve failed stage, downloaded bytes and restart action", async () => {
  const update = resource();
  let downloadAttempts = 0, installAttempts = 0, restartAttempts = 0, checks = 0;
  update.download = async () => { if (++downloadAttempts === 1) throw new Error("connection reset"); };
  update.install = async () => { if (++installAttempts === 1) throw new Error("installer failed"); };
  const { controller } = fixture("installed", {
    check: async () => { checks++; return update; },
    relaunch: async () => { if (++restartAttempts === 1) throw new Error("relaunch failed"); },
  });
  await controller.start(); await controller.check();
  await controller.download();
  assert.equal(controller.getSnapshot().info.recovery, "download");
  await controller.retry();
  assert.equal(controller.getSnapshot().info.phase, "downloaded");
  await controller.install();
  assert.equal(controller.getSnapshot().info.recovery, "install");
  await controller.retry();
  await controller.relaunch();
  assert.equal(controller.getSnapshot().info.recovery, "relaunch");
  await controller.retry();
  assert.equal(downloadAttempts, 2); assert.equal(installAttempts, 2); assert.equal(restartAttempts, 2);
  assert.equal(checks, 1);
  controller.stop();
});

test("only transient manual check failures retry; auto failures stay quiet without throttling future launches", async () => {
  let calls = 0;
  const { controller, state } = fixture("queryOnly", { check: async () => { calls++; throw new Error("timeout"); } });
  await controller.start(); await controller.check();
  assert.equal(calls, 2);
  assert.equal(controller.getSnapshot().info.kind, "timeout");
  assert.equal(state.touched, 0);
  controller.stop();
  const auto = fixture("installed", { check: async () => { throw new Error("signature invalid"); } });
  await auto.controller.start(); auto.auto(); await flush();
  assert.equal(auto.controller.getSnapshot().info.phase, "idle");
  assert.equal(auto.state.reports.length, 1);
  assert.equal(auto.state.touched, 0);
  auto.controller.stop();
});

test("policy initialization failures can retry without constructing a second controller", async () => {
  let attempts = 0;
  const { controller } = fixture("queryOnly", { loadPolicy: async () => {
    if (++attempts === 1) throw new Error("IPC unavailable");
    return { mode: "queryOnly", target: "windows-x86_64-nsis" };
  } });
  await controller.start();
  assert.equal(controller.getSnapshot().info.recovery, "initialize");
  await controller.retry(); await controller.check();
  assert.equal(controller.getSnapshot().info.phase, "latest");
  controller.stop();
});

test("catalog uniquely owns exact targets and query defaults; no static install permission remains", () => {
  const catalog = JSON.parse(readFileSync(new URL("../src-tauri/updater-targets.json", import.meta.url)));
  assert.equal(new Set(catalog.map(entry => entry.target)).size, catalog.length);
  assert.equal(new Set(catalog.map(entry => entry.suffix)).size, catalog.length);
  for (const entry of catalog) {
    assert.ok(entry.target.endsWith(`-${entry.bundle}`));
    assert.equal(catalog.filter(candidate => candidate.os === entry.os && candidate.arch === entry.arch && candidate.queryDefault).length, 1);
  }
  const cap = JSON.parse(readFileSync(new URL("../src-tauri/capabilities/default.json", import.meta.url)));
  assert.deepEqual(cap.permissions.filter(value => value.startsWith("updater:")), ["updater:allow-check"]);
  assert.equal(classifyUpdaterError(new Error("installer failed"), "install").kind, "install");
});
