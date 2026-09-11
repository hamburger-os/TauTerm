import { useState, useEffect, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

/**
 * 虚拟串口后端状态管理。
 *
 * UI 只消费能力状态与“确认属于 TauTerm 且当前无活跃 owner”的 orphan 数量；
 * 不根据系统中存在多少 com0com 端口自行推断残留资源。
 */
export function useCom0comStatus() {
  const [driverMissing, setDriverMissing] = useState(false);
  const [driverInstalling, setDriverInstalling] = useState(false);
  const [cleaningPorts, setCleaningPorts] = useState(false);
  const [orphanCount, setOrphanCount] = useState(0);

  const checkDriverStatus = useCallback(async () => {
    try {
      const status = await invoke<{
        files_present: boolean;
        driver_installed: boolean;
        orphan_count: number;
      }>("check_virtual_port_driver");
      setDriverMissing(status.files_present && !status.driver_installed);
      setOrphanCount(status.orphan_count ?? 0);
    } catch {
      // 不支持该能力的平台保持静默；平台后端负责给出功能可用性。
    }
  }, []);

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
      unlistenPromise.then(fn => fn());
    };
  }, []);

  // 端点资源的 owner 变化发生在会话断开生命周期中；事件后重新读取后端真相，
  // 不在前端猜测“应该减一/清零”。
  useEffect(() => {
    let cancelled = false;
    const unlistenPromise = listen("session-disconnected", () => {
      if (!cancelled) void checkDriverStatus();
    });
    return () => {
      cancelled = true;
      unlistenPromise.then(fn => fn());
    };
  }, [checkDriverStatus]);

  useEffect(() => {
    void checkDriverStatus();
  }, [checkDriverStatus]);

  const handleRetryVPort = useCallback(async () => {
    setDriverInstalling(true);
    try {
      await invoke<string>("install_virtual_port_driver");
    } catch (error) {
      console.warn("VPort driver installation failed:", error);
    } finally {
      setDriverInstalling(false);
      await checkDriverStatus();
    }
  }, [checkDriverStatus]);

  const handleCleanupVPorts = useCallback(async () => {
    setCleaningPorts(true);
    try {
      const result = await invoke<{ cleaned: number; message: string }>(
        "cleanup_virtual_ports"
      );
      console.log("VPort cleanup result:", result.message);
    } catch (error) {
      console.warn("VPort cleanup failed:", error);
    } finally {
      setCleaningPorts(false);
      await checkDriverStatus();
    }
  }, [checkDriverStatus]);

  return {
    driverMissing,
    driverInstalling,
    cleaningPorts,
    orphanCount,
    handleRetryVPort,
    handleCleanupVPorts,
  };
}
