import { useState, useCallback } from "react";
import { useTranslation } from "react-i18next";
import { SendBarProvider, useSendBar } from "./SendBarContext";
import BasicSend from "./BasicSend";
import CommandPanel from "./CommandPanel";
import Icon from "../common/Icon";
import type { SendBarMode } from "./types";
import styles from "./SendBar.module.css";

interface SendBarProps {
  sessionId: string;
}

/**
 * 发送栏容器组件
 *
 * - 左侧模式切换器：基础发送 / 指令面板
 * - 内容区：两个子视图始终挂载，通过 CSS display 切换可见性
 * - 高度由 App.tsx 通过 CSS 控制（支持拖拽调整）
 * - 状态由 SendBarContext 管理，切换视图时不会丢失输入数据
 */
export default function SendBar({ sessionId }: SendBarProps) {
  return (
    <SendBarProvider>
      <SendBarInner sessionId={sessionId} />
    </SendBarProvider>
  );
}

function SendBarInner({ sessionId }: SendBarProps) {
  const { t } = useTranslation();
  const { state, dispatch } = useSendBar();
  const { mode } = state;

  const [isChildRunning, setIsChildRunning] = useState(false);

  const handleModeChange = useCallback((newMode: SendBarMode) => {
    if (isChildRunning) return;
    dispatch({ type: "SET_MODE", mode: newMode });
  }, [isChildRunning, dispatch]);

  const handleSendingChange = useCallback((sending: boolean) => {
    setIsChildRunning(sending);
  }, []);

  const handleRunningChange = useCallback((running: boolean) => {
    setIsChildRunning(running);
  }, []);

  return (
    <div className={`${styles.container} liquid-glass`}>
      {/* 模式切换器 */}
      <div className={styles.modeSwitcher}>
        <button
          className={`${styles.modeBtn} ${mode === "basic" ? styles.modeBtnActive : ""}`}
          onClick={() => handleModeChange("basic")}
          disabled={isChildRunning}
          title={isChildRunning ? (t("sendBar.modeLocked") || "Mode locked during sending") : (t("sendBar.basicMode") || "Basic Send")}
        >
          <Icon name="upload" size="sm" />
        </button>
        <button
          className={`${styles.modeBtn} ${mode === "command" ? styles.modeBtnActive : ""}`}
          onClick={() => handleModeChange("command")}
          disabled={isChildRunning}
          title={isChildRunning ? (t("sendBar.modeLocked") || "Mode locked during sending") : (t("commandPanel.title") || "Command Panel")}
        >
          <Icon name="command-panel" size="sm" />
        </button>
      </div>

      {/* 内容区 — 两个视图始终挂载，CSS 显隐切换 */}
      <div className={styles.content}>
        <div className={mode === "basic" ? styles.wrapperVisible : styles.wrapperHidden}>
          <BasicSend
            sessionId={sessionId}
            isActive={mode === "basic"}
            onSendingChange={handleSendingChange}
          />
        </div>
        <div className={mode === "command" ? styles.wrapperVisible : styles.wrapperHidden}>
          <CommandPanel
            sessionId={sessionId}
            isActive={mode === "command"}
            onRunningChange={handleRunningChange}
          />
        </div>
      </div>
    </div>
  );
}
