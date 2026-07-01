import { useState, useCallback } from "react";
import { useTranslation } from "react-i18next";
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
 * - 内容区：根据当前模式渲染 BasicSend 或 CommandPanel
 * - 高度由 App.tsx 通过 CSS 控制（支持拖拽调整）
 */
export default function SendBar({ sessionId }: SendBarProps) {
  const { t } = useTranslation();

  const [mode, setMode] = useState<SendBarMode>("basic");
  const [isChildRunning, setIsChildRunning] = useState(false);

  const handleModeChange = useCallback((newMode: SendBarMode) => {
    if (isChildRunning) return;
    setMode(newMode);
  }, [isChildRunning]);

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

      {/* 内容区 */}
      <div className={styles.content}>
        {mode === "basic" ? (
          <BasicSend sessionId={sessionId} onSendingChange={handleSendingChange} />
        ) : (
          <CommandPanel sessionId={sessionId} onRunningChange={handleRunningChange} />
        )}
      </div>
    </div>
  );
}
