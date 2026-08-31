import { useState, useCallback, useEffect } from "react";
import { useTranslation } from "react-i18next";
import { listen } from "@tauri-apps/api/event";
import { SendBarProvider, useSendBar } from "./SendBarContext";
import BasicSend from "./BasicSend";
import CommandPanel from "./CommandPanel";
import AutoReplyPanel from "./AutoReplyPanel";
import ScriptEditor from "./ScriptEditor";
import TargetBar from "./TargetBar";
import Icon from "../common/Icon";
import type { IconName } from "../common/Icon";
import type { SendBarMode } from "./types";
import styles from "./SendBar.module.css";

interface SendBarProps {
  /** 容器会话 ID（目标路由与目标栏用；网络调试为容器，普通会话为标签页） */
  containerId: string;
  /** 脚本/自动应答引擎绑定的会话 ID（TCP 网络为选中对端；缺省等于 containerId） */
  engineSessionId?: string;
}

/**
 * 发送栏容器组件
 *
 * - 左侧模式切换器：基本发送 / 指令面板 / 自动应答 / 脚本编辑器
 * - 内容区：四个子视图始终挂载，通过 CSS display 切换可见性
 * - 高度由 App.tsx 通过 CSS 控制（支持拖拽调整）
 * - 状态由 SendBarContext 管理，切换视图时不会丢失输入数据
 */
export default function SendBar({ containerId, engineSessionId }: SendBarProps) {
  return (
    <SendBarProvider>
      <SendBarInner containerId={containerId} engineSessionId={engineSessionId} />
    </SendBarProvider>
  );
}

function SendBarInner({ containerId, engineSessionId }: SendBarProps) {
  const { t } = useTranslation();
  const { state, dispatch } = useSendBar();
  const { mode } = state;

  // 引擎绑定会话：TCP 网络为选中对端，其余为容器
  const engineId = engineSessionId ?? containerId;

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
      if (event.payload.session_id && event.payload.session_id !== engineId) return;
      dispatch({ type: "APPEND_SCRIPT_LOG", message: event.payload.message });
    });
    return () => { unlisten.then(fn => fn()); };
  }, [engineId, dispatch]);

  const modeButtons: { mode: SendBarMode; icon: IconName; title: string }[] = [
    { mode: "basic", icon: "send", title: t("sendBar.basicMode") },
    { mode: "command", icon: "commands", title: t("commandPanel.title") },
    { mode: "auto-reply", icon: "robot", title: t("sendBar.autoReplyMode") },
    { mode: "script", icon: "code", title: t("sendBar.scriptMode") },
  ];

  return (
    <div className={styles.container}>
      {/* 发送目标栏 — 网络调试（TCP server / UDP server）跨四模式共享，其余返回 null */}
      <TargetBar containerId={containerId} />

      {/* 主体：模式切换器（左） + 内容区（右），作为主发送栏卡片 */}
      <div className={`${styles.body} liquid-glass`}>
        {/* 模式切换器 */}
        <div className={styles.modeSwitcher}>
          {modeButtons.map((btn) => (
            <button
              key={btn.mode}
              className={`${styles.modeBtn} liquid-glass-button ${mode === btn.mode ? "active" : ""}`}
              onClick={() => handleModeChange(btn.mode)}
              disabled={isChildRunning}
              title={isChildRunning ? (t("sendBar.modeLocked")) : btn.title}
            >
              <Icon name={btn.icon} size="md" />
            </button>
          ))}
        </div>

        {/* 内容区 — 四个视图始终挂载，CSS 显隐切换 */}
        <div className={styles.content}>
          <div className={mode === "basic" ? styles.wrapperVisible : styles.wrapperHidden}>
            <BasicSend
              sessionId={containerId}
              isActive={mode === "basic"}
              onSendingChange={handleSendingChange}
            />
          </div>
          <div className={mode === "command" ? styles.wrapperVisible : styles.wrapperHidden}>
            <CommandPanel
              sessionId={containerId}
              isActive={mode === "command"}
              onRunningChange={handleRunningChange}
            />
          </div>
          <div className={mode === "auto-reply" ? styles.wrapperVisible : styles.wrapperHidden}>
            <AutoReplyPanel
              sessionId={engineId}
              isActive={mode === "auto-reply"}
              onRunningChange={handleRunningChange}
            />
          </div>
          <div className={mode === "script" ? styles.wrapperVisible : styles.wrapperHidden}>
            <ScriptEditor
              sessionId={engineId}
              isActive={mode === "script"}
              onRunningChange={handleRunningChange}
            />
          </div>
        </div>
      </div>
    </div>
  );
}
