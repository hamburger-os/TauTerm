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

/**
 * 更新器状态管理 Hook
 *
 * 封装 Tauri updater 的完整生命周期：检查 → 下载 → 安装 → 重启，
 * 以及 localStorage 持久化的检查频率管理。
 *
 * @param tr — i18n translate 函数，用于设置 `resultMessage` 本地化文本
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

  // ── 检查更新 ──
  const handleCheckUpdate = useCallback(async (isManual = false) => {
    setUpdateInfo({ phase: "checking" });
    try {
      const update = await check({ timeout: isManual ? 15000 : 10000 });
      // 只有真正从 updater endpoint 得到有效响应后才记录检查时间。
      // 网络/manifest/签名配置错误不能吞掉未来 24h 的自动重试机会。
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
    } catch (e) {
      console.error("[updater] check failed:", e);
      updateObjRef.current = null;
      if (isManual) {
        setUpdateInfo({ phase: "error", error: String(e) });
      } else {
        setUpdateInfo({ phase: "idle" }); // 自动检查静默失败，下次启动仍可重试
      }
    }
  }, [tr]);

  // ── 下载更新 ──
  const handleDownloadUpdate = useCallback(async () => {
    let update = updateObjRef.current;
    if (!update) {
      // 重新检查以获取 Update 对象，然后继续下载
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
      // Windows 成功启动 NSIS updater 后 Tauri 会直接退出进程；
      // macOS/Linux 会返回到这里，等待用户重启进入新版本。
      setUpdateInfo(prev => ({ ...prev, phase: "ready" }));
    } catch (e) {
      setUpdateInfo(prev => ({
        ...prev,
        phase: "error",
        error: String(e),
      }));
    }
  }, [handleCheckUpdate]);

  // ── 安装完成后重启（macOS/Linux）；Windows 正常更新时会在此前退出 ──
  const handleInstallUpdate = useCallback(async () => {
    try {
      await relaunch();
    } catch (e) {
      console.error("relaunch failed:", e);
    }
  }, []);

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
        handleCheckUpdate(false).catch(() => {
          /* 静默失败 */
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
