import type { CheckFrequency } from "../types/updater";

/** localStorage 键 — 检查频率设置 */
const CHECK_FREQUENCY_KEY = "tauterm-update-frequency";
/** localStorage 键 — 上次检查 Unix 时间戳 (ms) */
const LAST_CHECK_KEY = "tauterm-update-lastcheck";

/** 读取检查频率设置（默认 "daily"） */
export function getCheckFrequency(): CheckFrequency {
  try {
    const v = localStorage.getItem(CHECK_FREQUENCY_KEY);
    if (
      v === "always" ||
      v === "daily" ||
      v === "weekly" ||
      v === "never"
    ) {
      return v;
    }
  } catch {
    /* localStorage 不可用时回退 */
  }
  return "daily";
}

/** 写入检查频率设置 */
export function setCheckFrequency(freq: CheckFrequency): void {
  try {
    localStorage.setItem(CHECK_FREQUENCY_KEY, freq);
  } catch {
    /* noop */
  }
}

/** 记录本次检查时间戳 */
export function touchLastCheck(): void {
  try {
    localStorage.setItem(LAST_CHECK_KEY, Date.now().toString());
  } catch {
    /* noop */
  }
}

/** 按频率判断是否应执行自动检查 */
export function shouldAutoCheck(): boolean {
  const freq = getCheckFrequency();
  if (freq === "never") return false;
  if (freq === "always") return true;
  try {
    const lastStr = localStorage.getItem(LAST_CHECK_KEY);
    if (!lastStr) return true;
    const last = parseInt(lastStr, 10);
    if (isNaN(last)) return true;
    const interval = freq === "daily" ? 86_400_000 : 604_800_000;
    return Date.now() - last >= interval;
  } catch {
    return true;
  }
}
