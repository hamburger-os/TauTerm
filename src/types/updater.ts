import type { UpdaterErrorKind, UpdaterFailureStage } from "../utils/updaterError";

export type CheckFrequency = "always" | "daily" | "weekly" | "never";

/** Rust owns this decision; the UI never infers installation identity. */
export interface UpdaterPolicy {
  mode: "installed" | "queryOnly" | "unavailable";
  target: string | null;
}

export interface UpdateRelease {
  version: string;
  notes: string;
}

export type UpdateRecovery = "initialize" | "check" | "download" | "install" | "relaunch";

export type UpdateInfo =
  | { phase: "initializing" | "idle" | "latest" | "unavailable" | "checking" }
  | { phase: "available" | "downloaded" | "installing" | "restartRequired" | "restarting"; release: UpdateRelease }
  | { phase: "downloading"; release: UpdateRelease; downloadedBytes: number; totalBytes: number | null }
  | { phase: "error"; stage: UpdaterFailureStage; kind: UpdaterErrorKind; recovery: UpdateRecovery; release?: UpdateRelease };

export type UpdatePhase = UpdateInfo["phase"];

export interface UpdaterSnapshot {
  policy: UpdaterPolicy | null;
  info: UpdateInfo;
  frequency: CheckFrequency;
}
