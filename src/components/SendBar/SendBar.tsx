import { useState, useCallback, useEffect } from "react";
import { useTranslation } from "react-i18next";
import { listen } from "@tauri-apps/api/event";
import { SendBarProvider, useSendBar } from "./SendBarContext";
import BasicSend from "./BasicSend";
import CommandPanel from "./CommandPanel";
import AutoReplyPanel from "./AutoReplyPanel";
import ScriptEditor from "./ScriptEditor";
import Icon from "../common/Icon";
import type { SendBarMode } from "./types";
import styles from "./SendBar.module.css";

interface SendBarProps {
  sessionId: string;
}

/**
 * 发送栏容器组件
 *
 * - 左侧模式切换器：基本发送 / 指令面板 / 自动应答 / 脚本编辑器
 * - 内容区：四个子视图始终挂载，通过 CSS display 切换可见性
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

  // ── 共享脚本日志：始终监听 script-log，不依赖面板焦点 ──
  useEffect(() => {
    const unlisten = listen<{ session_id?: string; message: string }>("script-log", (event) => {
      if (event.payload.session_id && event.payload.session_id !== sessionId) return;
      dispatch({ type: "APPEND_SCRIPT_LOG", message: event.payload.message });
    });
    return () => { unlisten.then(fn => fn()); };
  }, [sessionId, dispatch]);

  const modeButtons: { mode: SendBarMode; icon: string; title: string }[] = [
    { mode: "basic", icon: "upload", title: t("sendBar.basicMode") },
    { mode: "command", icon: "command-panel", title: t("commandPanel.title") },
    { mode: "auto-reply", icon: "robot", title: t("sendBar.autoReplyMode") },
    { mode: "script", icon: "code", title: t("sendBar.scriptMode") },
  ];

  return (
    <div className={`${styles.container} liquid-glass`}>
      {/* 模式切换器 */}
      <div className={styles.modeSwitcher}>
        {modeButtons.map((btn) => (
          <button
            key={btn.mode}
            className={`${styles.modeBtn} ${mode === btn.mode ? styles.modeBtnActive : ""}`}
            onClick={() => handleModeChange(btn.mode)}
            disabled={isChildRunning}
            title={isChildRunning ? (t("sendBar.modeLocked")) : btn.title}
          >
            <Icon name={btn.icon} size="sm" />
          </button>
        ))}
      </div>

      {/* 内容区 — 四个视图始终挂载，CSS 显隐切换 */}
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
        <div className={mode === "auto-reply" ? styles.wrapperVisible : styles.wrapperHidden}>
          <AutoReplyPanel
            sessionId={sessionId}
            isActive={mode === "auto-reply"}
            onRunningChange={handleRunningChange}
          />
        </div>
        <div className={mode === "script" ? styles.wrapperVisible : styles.wrapperHidden}>
          <ScriptEditor
            sessionId={sessionId}
            isActive={mode === "script"}
            onRunningChange={handleRunningChange}
          />
        </div>
      </div>
    </div>
  );
}
