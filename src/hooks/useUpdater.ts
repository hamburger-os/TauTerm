import { useState, useCallback, useRef, useEffect } from "react";
import { check, type Update } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";
import type { UpdateInfo, CheckFrequency } from "../types/updater";
import {
  getCheckFrequency,
  setCheckFrequency,
  touchLastCheck,
  shouldAutoCheck,
} from "../utils/updater-store";
import { reportFrontendError } from "../utils/runtimeDiagnostics";
import {
  classifyUpdaterError,
  isRetryableUpdaterError,
  type UpdaterFailureStage,
} from "../utils/updaterError";

interface UseUpdaterReturn {
  updateInfo: UpdateInfo;
  updateCheckFrequency: CheckFrequency;
  handleCheckUpdate: (isManual?: boolean) => Promise<void>;
  handleDownloadUpdate: () => Promise<void>;
  handleInstallUpdate: () => Promise<void>;
  handleCheckFrequencyChange: (freq: CheckFrequency) => void;
  handleVersionClick: () => void;
  settingsOpen: boolean;
  setSettingsOpen: (open: boolean) => void;
  settingsInitialCategory: string | null;
  setSettingsInitialCategory: (cat: string | null) => void;
}

const MANUAL_CHECK_TIMEOUT_MS = 30_000;
const AUTO_CHECK_TIMEOUT_MS = 15_000;
const MANUAL_RETRY_DELAY_MS = 750;

function sleep(ms: number): Promise<void> {
  return new Promise(resolve => setTimeout(resolve, ms));
}

/**
 * 更新器状态管理 Hook
 *
 * 封装 Tauri updater 的完整生命周期：检查 → 下载 → 安装 → 重启，
 * 以及 localStorage 持久化的检查频率管理。
 *
 * 原始 updater 错误只进入统一运行时诊断日志；UI 仅展示稳定、可操作的
 * 本地化错误类别，避免把 reqwest/TLS 内部错误直接暴露给用户。
 *
 * @param tr — i18n translate 函数，用于设置本地化状态文本
 */
export function useUpdater(
  tr: (key: string, options?: Record<string, unknown>) => string,
): UseUpdaterReturn {
  const [updateInfo, setUpdateInfo] = useState<UpdateInfo>({ phase: "idle" });
  const [updateCheckFrequency, setUpdateCheckFrequency] =
    useState<CheckFrequency>(getCheckFrequency);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [settingsInitialCategory, setSettingsInitialCategory] = useState<
    string | null
  >(null);

  /** 保存 check() 返回的 Update 对象，用于后续 downloadAndInstall() */
  const updateObjRef = useRef<Update | null>(null);

  const recordFailure = useCallback((
    stage: UpdaterFailureStage,
    error: unknown,
    context?: string,
  ) => {
    const failure = classifyUpdaterError(error, stage);
    const details = [`stage=${stage}`, `kind=${failure.kind}`];
    if (context) details.push(context);
    reportFrontendError(`updater.${stage}`, error, details.join("; "));
    return failure;
  }, []);

  // ── 检查更新 ──
  const handleCheckUpdate = useCallback(async (isManual = false) => {
    setUpdateInfo({ phase: "checking" });
    const maxAttempts = isManual ? 2 : 1;
    const timeout = isManual ? MANUAL_CHECK_TIMEOUT_MS : AUTO_CHECK_TIMEOUT_MS;

    for (let attempt = 1; attempt <= maxAttempts; attempt += 1) {
      try {
        const update = await check({ timeout });
        // 只有真正从 updater endpoint 得到有效响应后才记录检查时间。
        // 网络/manifest/签名配置错误不能吞掉未来的自动重试机会。
        touchLastCheck();

        if (update) {
          updateObjRef.current = update;
          setUpdateInfo({
            phase: "available",
            latestVersion: update.version,
            releaseNotes: update.body ?? "",
          });
        } else {
          updateObjRef.current = null;
          setUpdateInfo({
            phase: "idle",
            resultMessage: isManual ? tr("updater.alreadyLatest") : undefined,
          });
        }
        return;
      } catch (error) {
        const failure = recordFailure(
          "check",
          error,
          `manual=${isManual}; attempt=${attempt}/${maxAttempts}`,
        );
        updateObjRef.current = null;

        if (
          isManual &&
          attempt < maxAttempts &&
          isRetryableUpdaterError(failure.kind)
        ) {
          await sleep(MANUAL_RETRY_DELAY_MS);
          continue;
        }

        if (isManual) {
          setUpdateInfo({
            phase: "error",
            error: tr(`updaterError.${failure.kind}`),
          });
        } else {
          // 自动检查静默失败；不更新 last-check，后续启动仍可重试。
          setUpdateInfo({ phase: "idle" });
        }
        return;
      }
    }
  }, [recordFailure, tr]);

  // ── 下载并安装更新 ──
  const handleDownloadUpdate = useCallback(async () => {
    let update = updateObjRef.current;
    if (!update) {
      // 重新检查以获取 Update 对象，然后继续下载。
      await handleCheckUpdate(false);
      update = updateObjRef.current;
    }
    if (!update) return; // 仍无可用更新（已是最新版本或检查失败）

    setUpdateInfo(prev => ({
      ...prev,
      phase: "downloading",
      downloadedBytes: 0,
      totalBytes: 0,
    }));
    try {
      await update.downloadAndInstall(event => {
        switch (event.event) {
          case "Started":
            setUpdateInfo(prev => ({
              ...prev,
              totalBytes: event.data.contentLength ?? 0,
            }));
            break;
          case "Progress":
            setUpdateInfo(prev => ({
              ...prev,
              downloadedBytes:
                (prev.downloadedBytes ?? 0) + event.data.chunkLength,
            }));
            break;
          case "Finished":
            break;
        }
      });
      // Windows 上系统安装器接管后应用可能直接退出；其它平台会返回到这里。
      setUpdateInfo(prev => ({ ...prev, phase: "ready" }));
    } catch (error) {
      const failure = recordFailure("download-install", error);
      setUpdateInfo(prev => ({
        ...prev,
        phase: "error",
        error: tr(`updaterError.${failure.kind}`),
      }));
    }
  }, [handleCheckUpdate, recordFailure, tr]);

  // ── 安装完成后重启；若系统安装器已接管流程，应用会在此前退出 ──
  const handleInstallUpdate = useCallback(async () => {
    try {
      await relaunch();
    } catch (error) {
      const failure = recordFailure("relaunch", error);
      setUpdateInfo(prev => ({
        ...prev,
        phase: "error",
        error: tr(`updaterError.${failure.kind}`),
      }));
    }
  }, [recordFailure, tr]);

  // ── 频率变更 ──
  const handleCheckFrequencyChange = useCallback((freq: CheckFrequency) => {
    setUpdateCheckFrequency(freq);
    setCheckFrequency(freq);
  }, []);

  // ── 点击版本号 → About 页 ──
  const handleVersionClick = useCallback(() => {
    setSettingsInitialCategory("about");
    setSettingsOpen(true);
  }, []);

  // ── 启动时自动检查 ──
  useEffect(() => {
    if (shouldAutoCheck()) {
      const timer = setTimeout(() => {
        handleCheckUpdate(false).catch(error => {
          // handleCheckUpdate 已处理预期 updater 错误；这里只兜底 Hook 外异常。
          reportFrontendError("updater.autocheck", error);
        });
      }, 3000);
      return () => clearTimeout(timer);
    }
  }, [handleCheckUpdate]);

  return {
    updateInfo,
    updateCheckFrequency,
    handleCheckUpdate,
    handleDownloadUpdate,
    handleInstallUpdate,
    handleCheckFrequencyChange,
    handleVersionClick,
    settingsOpen,
    setSettingsOpen,
    settingsInitialCategory,
    setSettingsInitialCategory,
  };
}
