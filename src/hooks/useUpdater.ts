import { useState, useCallback, useEffect, useSyncExternalStore } from "react";
import { invoke } from "@tauri-apps/api/core";
import { check } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";
import { UpdaterController } from "../core/updater-controller";
import { getCheckFrequency, setCheckFrequency, touchLastCheck, shouldAutoCheck } from "../utils/updater-store";
import { reportFrontendError } from "../utils/runtimeDiagnostics";
import type { UpdaterPolicy } from "../types/updater";

/** React adapter; lifecycle state and resources belong to the controller. */
export function useUpdater() {
  const [controller] = useState(() => new UpdaterController({
    loadPolicy: () => invoke<UpdaterPolicy>("get_updater_policy"),
    check,
    relaunch,
    report: (stage, error, context) => reportFrontendError(`updater.${stage}`, error, context),
    getFrequency: getCheckFrequency,
    setFrequency: setCheckFrequency,
    touchLastCheck,
    shouldAutoCheck,
    schedule: (callback, ms) => {
      const timer = setTimeout(callback, ms);
      return () => clearTimeout(timer);
    },
    delay: ms => new Promise(resolve => setTimeout(resolve, ms)),
  }));
  const snapshot = useSyncExternalStore(controller.subscribe, controller.getSnapshot);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [settingsInitialCategory, setSettingsInitialCategory] = useState<string | null>(null);

  useEffect(() => {
    void controller.start();
    return controller.stop;
  }, [controller]);

  const handleVersionClick = useCallback(() => {
    setSettingsInitialCategory("about");
    setSettingsOpen(true);
  }, []);

  return {
    updateInfo: snapshot.info,
    updaterPolicy: snapshot.policy,
    updateCheckFrequency: snapshot.frequency,
    handleCheckUpdate: controller.check,
    handleDownloadUpdate: controller.download,
    handleInstallUpdate: controller.install,
    handleRestartUpdate: controller.relaunch,
    handleRetryUpdate: controller.retry,
    handleCheckFrequencyChange: controller.setFrequency,
    handleVersionClick,
    settingsOpen, setSettingsOpen, settingsInitialCategory, setSettingsInitialCategory,
  };
}
