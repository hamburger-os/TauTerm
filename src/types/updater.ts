/** 更新检查频率 */
export type CheckFrequency = "always" | "daily" | "weekly" | "never";

/** 更新阶段 */
export type UpdatePhase =
  | "idle"
  | "checking"
  | "available"
  | "downloading"
  | "ready"
  | "error";

/** 更新状态信息 */
export interface UpdateInfo {
  phase: UpdatePhase;
  /** 最新版本号 */
  latestVersion?: string;
  /** 更新日志 / Release Notes */
  releaseNotes?: string;
  /** 已下载字节数 */
  downloadedBytes?: number;
  /** 总字节数 */
  totalBytes?: number;
  /** 错误消息 */
  error?: string;
  /** 上次手动检查的结果提示（如"已是最新版本"） */
  resultMessage?: string;
}
