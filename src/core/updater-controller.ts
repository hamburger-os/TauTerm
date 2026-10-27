import type { Update, DownloadEvent } from "@tauri-apps/plugin-updater";
import type {
  CheckFrequency, UpdaterPolicy, UpdaterSnapshot, UpdateRelease, UpdateRecovery,
} from "../types/updater.ts";
import {
  classifyUpdaterError, isRetryableUpdaterError, type UpdaterFailureStage,
} from "../utils/updaterError.ts";

export type UpdateResource = Pick<Update, "version" | "body" | "download" | "install" | "close">;

interface UpdaterDependencies {
  loadPolicy: () => Promise<UpdaterPolicy>;
  check: (options: { target: string; timeout: number }) => Promise<UpdateResource | null>;
  relaunch: () => Promise<void>;
  report: (stage: string, error: unknown, context?: string) => void;
  getFrequency: () => CheckFrequency;
  setFrequency: (frequency: CheckFrequency) => void;
  shouldAutoCheck: () => boolean;
  touchLastCheck: () => void;
  schedule: (callback: () => void, ms: number) => () => void;
  delay: (ms: number) => Promise<void>;
}

interface Operation {
  kind: UpdateRecovery;
  manual: boolean;
  promise: Promise<void>;
}

const MANUAL_CHECK_TIMEOUT_MS = 30_000;
const AUTO_CHECK_TIMEOUT_MS = 15_000;
const DOWNLOAD_TIMEOUT_MS = 300_000;

/** One application-scoped owner for requests, resources and lifecycle state. */
export class UpdaterController {
  private snapshot: UpdaterSnapshot;
  private readonly listeners = new Set<() => void>();
  private readonly closed = new WeakSet<UpdateResource>();
  private active = false;
  private operation: Operation | null = null;
  private resource: UpdateResource | null = null;
  private cancelAuto: (() => void) | null = null;
  private autoAttempted = false;
  private readonly deps: UpdaterDependencies;

  constructor(deps: UpdaterDependencies) {
    this.deps = deps;
    this.snapshot = { policy: null, info: { phase: "initializing" }, frequency: deps.getFrequency() };
  }

  getSnapshot = (): UpdaterSnapshot => this.snapshot;
  subscribe = (listener: () => void): (() => void) => {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  };

  private emit(): void {
    for (const listener of this.listeners) listener();
  }

  private set(info: UpdaterSnapshot["info"]): void {
    this.snapshot = { ...this.snapshot, info };
    this.emit();
  }

  private current(op: Operation): boolean {
    return this.active && this.operation === op;
  }

  private async close(resource: UpdateResource | null): Promise<void> {
    if (!resource || this.closed.has(resource)) return;
    this.closed.add(resource);
    try {
      await resource.close();
    } catch (error) {
      this.deps.report("close", error);
    }
  }

  private run(kind: UpdateRecovery, task: (op: Operation) => Promise<void>): Promise<void> {
    if (!this.active || this.operation) return Promise.resolve();
    const op: Operation = { kind, manual: false, promise: Promise.resolve() };
    this.operation = op;
    op.promise = task(op).finally(() => {
      if (this.operation === op) this.operation = null;
    });
    return op.promise;
  }

  start = (): Promise<void> => {
    if (this.active) return Promise.resolve();
    this.active = true;
    this.autoAttempted = false;
    return this.initialize();
  };

  stop = (): void => {
    this.active = false;
    this.cancelAuto?.();
    this.cancelAuto = null;
    const resource = this.resource;
    const pending = this.operation;
    this.resource = null;
    this.operation = null;
    // Native operations must finish before their resource can be closed.
    if (pending) void pending.promise.then(() => this.close(resource));
    else void this.close(resource);
  };

  private initialize = (): Promise<void> => this.run("initialize", async op => {
    this.set({ phase: "initializing" });
    try {
      const policy = await this.deps.loadPolicy();
      if (!this.current(op)) return;
      this.snapshot = { ...this.snapshot, policy };
      this.set({ phase: policy.mode === "unavailable" ? "unavailable" : "idle" });
      this.scheduleAuto();
    } catch (error) {
      this.fail(op, "policy", error, "initialize");
    }
  });

  private scheduleAuto(): void {
    this.cancelAuto?.();
    this.cancelAuto = null;
    if (
      !this.active || this.autoAttempted || this.snapshot.policy?.mode !== "installed"
      || !this.deps.shouldAutoCheck()
    ) return;
    this.cancelAuto = this.deps.schedule(() => {
      this.cancelAuto = null;
      if (!this.active || this.autoAttempted || !this.deps.shouldAutoCheck()) return;
      this.autoAttempted = true;
      void this.check(false);
    }, 3000);
  }

  setFrequency = (frequency: CheckFrequency): void => {
    if (this.snapshot.policy?.mode !== "installed") return;
    this.deps.setFrequency(frequency);
    this.snapshot = { ...this.snapshot, frequency };
    this.emit();
    this.scheduleAuto();
  };

  check = (manual = true): Promise<void> => {
    const policy = this.snapshot.policy;
    const target = policy?.target;
    if (!target || policy.mode === "unavailable") return Promise.resolve();
    if (this.operation?.kind === "check") {
      this.operation.manual ||= manual;
      return this.operation.promise;
    }
    if (!["idle", "latest", "available", "error"].includes(this.snapshot.info.phase)) return Promise.resolve();
    if (manual) {
      this.cancelAuto?.();
      this.cancelAuto = null;
      this.autoAttempted = true;
    }
    return this.run("check", async op => {
      op.manual = manual;
      this.set({ phase: "checking" });
      const old = this.resource;
      this.resource = null;
      await this.close(old);
      for (let attempt = 1; this.current(op); attempt++) {
        try {
          const update = await this.deps.check({
            target,
            timeout: op.manual ? MANUAL_CHECK_TIMEOUT_MS : AUTO_CHECK_TIMEOUT_MS,
          });
          if (!this.current(op)) {
            await this.close(update);
            return;
          }
          this.deps.touchLastCheck();
          if (update) {
            const release = { version: update.version, notes: update.body ?? "" };
            if (policy.mode === "installed") this.resource = update;
            else await this.close(update);
            if (this.current(op)) this.set({ phase: "available", release });
          } else this.set({ phase: op.manual ? "latest" : "idle" });
          return;
        } catch (error) {
          const failure = classifyUpdaterError(error, "check");
          this.deps.report("check", error, `manual=${op.manual}; attempt=${attempt}; kind=${failure.kind}`);
          if (!this.current(op)) return;
          if (op.manual && attempt < 2 && isRetryableUpdaterError(failure.kind)) {
            await this.deps.delay(750);
            continue;
          }
          this.set(op.manual
            ? { phase: "error", stage: "check", kind: failure.kind, recovery: "check" }
            : { phase: "idle" });
          return;
        }
      }
    });
  };

  download = (): Promise<void> => {
    const { info, policy } = this.snapshot;
    const resource = this.resource;
    if (
      policy?.mode !== "installed" || !resource
      || !(info.phase === "available" || (info.phase === "error" && info.recovery === "download"))
    ) return Promise.resolve();
    const release: UpdateRelease = { version: resource.version, notes: resource.body ?? "" };
    return this.run("download", async op => {
      this.set({ phase: "downloading", release, downloadedBytes: 0, totalBytes: null });
      try {
        await resource.download((event: DownloadEvent) => {
          if (!this.current(op) || this.snapshot.info.phase !== "downloading") return;
          const progress = this.snapshot.info;
          if (event.event === "Started") {
            this.set({ ...progress, totalBytes: event.data.contentLength || null });
          }
          if (event.event === "Progress") {
            this.set({ ...progress, downloadedBytes: progress.downloadedBytes + event.data.chunkLength });
          }
        }, { timeout: DOWNLOAD_TIMEOUT_MS });
        if (this.current(op)) this.set({ phase: "downloaded", release });
      } catch (error) {
        this.fail(op, "download", error, "download", release);
      }
    });
  };

  install = (): Promise<void> => {
    const { info, policy } = this.snapshot;
    const resource = this.resource;
    if (
      policy?.mode !== "installed" || !resource
      || !(info.phase === "downloaded" || (info.phase === "error" && info.recovery === "install"))
    ) return Promise.resolve();
    const release = { version: resource.version, notes: resource.body ?? "" };
    return this.run("install", async op => {
      this.set({ phase: "installing", release });
      try {
        // Windows hands off to the installer and exits. Other platforms return here.
        await resource.install();
        if (this.current(op)) {
          this.resource = null;
          await this.close(resource);
          if (this.current(op)) this.set({ phase: "restartRequired", release });
        }
      } catch (error) {
        this.fail(op, "install", error, "install", release);
      }
    });
  };

  relaunch = (): Promise<void> => {
    const info = this.snapshot.info;
    if (
      this.snapshot.policy?.mode !== "installed"
      || !(info.phase === "restartRequired" || (info.phase === "error" && info.recovery === "relaunch"))
      || !info.release
    ) return Promise.resolve();
    const release = info.release;
    return this.run("relaunch", async op => {
      this.set({ phase: "restarting", release });
      try {
        await this.deps.relaunch();
      } catch (error) {
        this.fail(op, "relaunch", error, "relaunch", release);
      }
    });
  };

  retry = (): Promise<void> => {
    const info = this.snapshot.info;
    if (info.phase !== "error") return Promise.resolve();
    switch (info.recovery) {
      case "initialize": return this.initialize();
      case "check": return this.check();
      case "download": return this.download();
      case "install": return this.install();
      case "relaunch": return this.relaunch();
    }
  };

  private fail(
    op: Operation, stage: UpdaterFailureStage, error: unknown,
    recovery: UpdateRecovery, release?: UpdateRelease,
  ): void {
    const failure = classifyUpdaterError(error, stage);
    this.deps.report(stage, error, `kind=${failure.kind}`);
    if (this.current(op)) this.set({ phase: "error", stage, kind: failure.kind, recovery, release });
  }
}
