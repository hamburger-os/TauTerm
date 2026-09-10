import { useCallback, useEffect } from "react";
import { useTranslation } from "react-i18next";
import { listen } from "@tauri-apps/api/event";
import { SendBarProvider, useSendBar } from "./SendBarContext";
import BasicSend from "./BasicSend";
import CommandPanel from "./CommandPanel";
import AutoReplyPanel from "./AutoReplyPanel";
import ScriptEditor from "./ScriptEditor";
import TargetBar from "./TargetBar";
import { useNetworkSendTargetSync } from "./useNetworkSendTargetSync";
import Icon from "../common/Icon";
import type { IconName } from "../common/Icon";
import type { SendBarMode } from "./types";
import styles from "./SendBar.module.css";

interface SendBarProps {
  /** 当前发送栏所属会话 ID，也是脚本/自动应答运行时的绑定 ID。 */
  containerId: string;
}

/**
 * 发送栏容器组件。
 *
 * 每个会话保留自己的 SendBarProvider，从而保留草稿、选择与执行所有权；四个模式不再全部常驻 DOM，
 * 仅挂载当前模式。工程资产仍通过 AssetStore 跨会话共享。
 */
export default function SendBar({ containerId }: SendBarProps) {
  return (
    <SendBarProvider>
      <SendBarInner containerId={containerId} />
    </SendBarProvider>
  );
}

function SendBarInner({ containerId }: SendBarProps) {
  const { t } = useTranslation();
  const { state, dispatch } = useSendBar();
  const { mode, executionMode } = state;

  useNetworkSendTargetSync(containerId);

  const handleModeChange = useCallback((newMode: SendBarMode) => {
    if (executionMode !== null) return;
    dispatch({ type: "SET_MODE", mode: newMode });
  }, [executionMode, dispatch]);

  const handleExecutionChange = useCallback((owner: SendBarMode, running: boolean) => {
    dispatch({ type: "SET_EXECUTION_MODE", owner, running });
  }, [dispatch]);

  // ── 共享脚本日志：始终监听 script-log，不依赖面板焦点 ──
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
      <TargetBar containerId={containerId} />

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
