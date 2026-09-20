import { useCallback, useEffect } from "react";
import { useTranslation } from "react-i18next";
import { listen } from "@tauri-apps/api/event";
import { pluginRegistry } from "../../core/plugin-registry";
import { useSession } from "../../context/SessionContext";
import { SendBarProvider, useSendBar } from "./SendBarContext";
import BasicSend from "./BasicSend";
import CommandPanel from "./CommandPanel";
import AutoReplyPanel from "./AutoReplyPanel";
import ScriptEditor from "./ScriptEditor";
import Icon from "../common/Icon";
import type { IconName } from "../common/Icon";
import type { SendBarMode } from "./types";
import styles from "./SendBar.module.css";

interface SendBarProps {
  containerId: string;
  showTargetBar: boolean;
}

export default function SendBar({ containerId, showTargetBar }: SendBarProps) {
  return (
    <SendBarProvider>
      <SendBarInner containerId={containerId} showTargetBar={showTargetBar} />
    </SendBarProvider>
  );
}

function SendBarInner({ containerId, showTargetBar }: SendBarProps) {
  const { t } = useTranslation();
  const { state, dispatch } = useSendBar();
  const { state: sessionState } = useSession();
  const { mode, executionMode } = state;
  const tab = sessionState.tabs.find(item => item.id === containerId);
  const SendTarget = tab ? pluginRegistry.get(tab.pluginId)?.sendTarget : undefined;

  const handleModeChange = useCallback((newMode: SendBarMode) => {
    if (executionMode !== null) return;
    dispatch({ type: "SET_MODE", mode: newMode });
  }, [executionMode, dispatch]);

  const handleExecutionChange = useCallback((owner: SendBarMode, running: boolean) => {
    dispatch({ type: "SET_EXECUTION_MODE", owner, running });
  }, [dispatch]);

  useEffect(() => {
    const unlisten = listen<{ session_id?: string; message: string }>("script-log", (event) => {
      if (event.payload.session_id && event.payload.session_id !== containerId) return;
      dispatch({ type: "APPEND_SCRIPT_LOG", message: event.payload.message });
    });
    return () => { unlisten.then(fn => fn()); };
  }, [containerId, dispatch]);

  const modeButtons: { mode: SendBarMode; icon: IconName; title: string }[] = [
    { mode: "basic", icon: "send", title: t("sendBar.basicMode") },
    { mode: "command", icon: "commands", title: t("commandPanel.title") },
    { mode: "auto-reply", icon: "robot", title: t("sendBar.autoReplyMode") },
    { mode: "script", icon: "code", title: t("sendBar.scriptMode") },
  ];

  return (
    <div className={styles.container}>
      {showTargetBar && SendTarget && (
        <SendTarget sessionId={containerId} disabled={executionMode !== null} />
      )}

      <div className={`${styles.body} liquid-glass-panel`}>
        <div className={styles.modeSwitcher}>
          {modeButtons.map(btn => (
            <button
              key={btn.mode}
              className={`${styles.modeBtn} liquid-glass-button ${mode === btn.mode ? "liquid-theme-selected" : ""}`}
              onClick={() => handleModeChange(btn.mode)}
              disabled={executionMode !== null}
              aria-pressed={mode === btn.mode}
              title={executionMode !== null ? t("sendBar.modeLocked") : btn.title}
            >
              <Icon name={btn.icon} size="md" />
            </button>
          ))}
        </div>

        <div className={styles.content}>
          {mode === "basic" && (
            <div className={styles.wrapperVisible}>
              <BasicSend
                sessionId={containerId}
                isActive
                onSendingChange={running => handleExecutionChange("basic", running)}
              />
            </div>
          )}
          {mode === "command" && (
            <div className={styles.wrapperVisible}>
              <CommandPanel
                sessionId={containerId}
                isActive
                onRunningChange={running => handleExecutionChange("command", running)}
              />
            </div>
          )}
          {mode === "auto-reply" && (
            <div className={styles.wrapperVisible}>
              <AutoReplyPanel
                sessionId={containerId}
                isActive
                onRunningChange={running => handleExecutionChange("auto-reply", running)}
              />
            </div>
          )}
          {mode === "script" && (
            <div className={styles.wrapperVisible}>
              <ScriptEditor
                sessionId={containerId}
                isActive
                onRunningChange={running => handleExecutionChange("script", running)}
              />
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
