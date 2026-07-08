import { useState, useEffect, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

/**
 * com0com 虚拟串口驱动状态管理 hook。
 *
 * 集中管理驱动检测、事件监听、安装/清理操作的异步状态，
 * 供 StatusBar 等展示组件消费。
 */
export function useCom0comStatus() {
  // ── State ──────────────────────────────────────────

  const [driverMissing, setDriverMissing] = useState(false);
  const [driverInstalling, setDriverInstalling] = useState(false);
  const [cleaningPorts, setCleaningPorts] = useState(false);
  const [orphanCount, setOrphanCount] = useState(0);

  // ── Driver status check ────────────────────────────

  const checkDriverStatus = useCallback(async () => {
    try {
      const status = await invoke<{
        files_present: boolean;
        driver_installed: boolean;
        orphan_count: number;
      }>("check_virtual_port_driver");
      if (status.files_present && !status.driver_installed) {
        setDriverMissing(true);
      }
      setOrphanCount(status.orphan_count ?? 0);
    } catch {
      // Non-Windows platforms may not support this command, silently ignore
    }
  }, []);

  // ── Event listeners ────────────────────────────────

  // Listen for backend driver-missing notifications (runtime status changes)
  useEffect(() => {
    let cancelled = false;
    const unlistenPromise = listen<{ reason: string; can_install: boolean }>(
      "com0com-driver-missing",
      () => {
        if (!cancelled) setDriverMissing(true);
      }
    );
    return () => {
      cancelled = true;
      unlistenPromise.then((fn) => fn());
    };
  }, []);

  // Re-check driver/orphan status after session disconnect
  // (destroy_pair may leave orphans when lacking admin privileges)
  useEffect(() => {
    let cancelled = false;
    const unlistenPromise = listen("session-disconnected", () => {
      if (!cancelled) checkDriverStatus();
    });
    return () => {
      cancelled = true;
      unlistenPromise.then((fn) => fn());
    };
  }, [checkDriverStatus]);

  // Proactive driver check on mount (handles race where event fires before
  // component subscribes)
  useEffect(() => {
    checkDriverStatus();
  }, [checkDriverStatus]);

  // ── Actions ────────────────────────────────────────

  const handleRetryVPort = useCallback(async () => {
    setDriverInstalling(true);
    try {
      const result = await invoke<string>("install_virtual_port_driver");
      if (result === "installed" || result === "already_installed") {
        setDriverMissing(false);
      }
    } catch (e) {
      console.warn("VPort driver installation failed:", e);
    } finally {
      setDriverInstalling(false);
    }
  }, []);

  const handleCleanupVPorts = useCallback(async () => {
    setCleaningPorts(true);
    try {
      const result = await invoke<{ cleaned: number; message: string }>(
        "cleanup_virtual_ports"
      );
      console.log("VPort cleanup result:", result.message);
      if (result.cleaned > 0) {
        setOrphanCount(0);
      }
    } catch (e) {
      console.warn("VPort cleanup failed:", e);
    } finally {
      setCleaningPorts(false);
    }
  }, []);

  return {
    driverMissing,
    driverInstalling,
    cleaningPorts,
    orphanCount,
    handleRetryVPort,
    handleCleanupVPorts,
  };
}
