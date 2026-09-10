import { useState, useCallback, useEffect, useRef } from "react";
import { createPortal } from "react-dom";
import { useTranslation } from "react-i18next";
import { useSession } from "../../context/SessionContext";
import { useToast } from "../../context/ToastContext";
import Icon from "../common/Icon";
import type { NewlineMode } from "./types";
import { useSendBar } from "./SendBarContext";
import { buildSendPayload, isHexInputValid } from "./sendPayload";
import styles from "./BasicSend.module.css";

interface BasicSendProps {
  sessionId: string;
  isActive: boolean;
  onSendingChange?: (sending: boolean) => void;
}

/**
 * 基础发送面板
 *
 * 支持文本/HEX 输入、换行符追加、重复发送、发送历史。
 * 手动发送与重复发送共用同一 payload 编码入口；重复发送严格串行，
 * 下一次发送仅在上一次 sendToTarget 完成后才开始计时。
 */
export default function BasicSend({ sessionId, isActive, onSendingChange }: BasicSendProps) {
  const { t } = useTranslation();
  const { sendToTarget, isSessionConnected } = useSession();
  const { showToast } = useToast();

  const { state: sendBarState, dispatch } = useSendBar();
  const {
    inputText,
    newlineMode,
    sendMode,
    repeatEnabled,
    repeatInterval,
    sendHistory,
  } = sendBarState.basic;

  const [showOptions, setShowOptions] = useState(false);
  const [dropdownStyle, setDropdownStyle] = useState<React.CSSProperties>({});

  const inputRef = useRef<HTMLTextAreaElement>(null);
  const repeatTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const repeatRunIdRef = useRef(0);
  const historyBtnRef = useRef<HTMLButtonElement>(null);
  const inputRefForRepeat = useRef(inputText);
  inputRefForRepeat.current = inputText;

  // 网络调试对端经 peerSessions 注册表判定；普通会话走 tabs
  const isConnected = isSessionConnected(sessionId);

  const doSend = useCallback(async () => {
    if (!isConnected) return;
    const currentInput = inputRefForRepeat.current;
    const data = buildSendPayload(currentInput, sendMode, newlineMode);
    if (data === null) return;

    try {
      await sendToTarget(sessionId, data);
      dispatch({ type: "ADD_SEND_HISTORY", entry: currentInput });
    } catch (e) {
      showToast("error", String(e));
    } finally {
      inputRef.current?.focus();
    }
  }, [isConnected, newlineMode, sendMode, sessionId, sendToTarget, showToast, dispatch]);

  // 重复发送：使用 self-scheduling timeout 保证背压。底层写入未完成时绝不重叠发送。
  useEffect(() => {
    repeatRunIdRef.current += 1;
    const runId = repeatRunIdRef.current;

    if (repeatTimerRef.current) {
      clearTimeout(repeatTimerRef.current);
      repeatTimerRef.current = null;
    }

    if (!isActive || !isConnected || !repeatEnabled || repeatInterval < 50) return;

    const initialPayload = buildSendPayload(inputRefForRepeat.current, sendMode, newlineMode);
    if (initialPayload !== null) {
      dispatch({ type: "ADD_SEND_HISTORY", entry: inputRefForRepeat.current });
    }

    const scheduleNext = () => {
      if (repeatRunIdRef.current !== runId) return;
      repeatTimerRef.current = setTimeout(() => { void sendOnce(); }, repeatInterval);
    };

    const sendOnce = async () => {
      if (repeatRunIdRef.current !== runId) return;
      const currentInput = inputRefForRepeat.current;
      const data = buildSendPayload(currentInput, sendMode, newlineMode);

      if (data !== null) {
        try {
          await sendToTarget(sessionId, data);
        } catch (e) {
          if (repeatRunIdRef.current === runId) {
            repeatRunIdRef.current += 1;
            dispatch({ type: "SET_REPEAT_ENABLED", enabled: false });
            showToast("error", String(e));
          }
          return;
        }
      }

      scheduleNext();
    };

    scheduleNext();

    return () => {
      if (repeatRunIdRef.current === runId) repeatRunIdRef.current += 1;
      if (repeatTimerRef.current) {
        clearTimeout(repeatTimerRef.current);
        repeatTimerRef.current = null;
      }
    };
  }, [
    isActive,
    isConnected,
    repeatEnabled,
    repeatInterval,
    sendMode,
    newlineMode,
    sessionId,
    sendToTarget,
    showToast,
    dispatch,
  ]);

  // 断开会话时重置
  const prevConnectedRef = useRef(isConnected);
  useEffect(() => {
    const wasConnected = prevConnectedRef.current;
    prevConnectedRef.current = isConnected;
    if (wasConnected && !isConnected) {
      dispatch({ type: "RESET_BASIC" });
      setShowOptions(false);
    }
  }, [isConnected, dispatch]);

  // 通知父组件重复发送状态（用于锁定模式切换）
  const onSendingChangeRef = useRef(onSendingChange);
  onSendingChangeRef.current = onSendingChange;
  useEffect(() => {
    if (repeatEnabled && isConnected && isActive) {
      onSendingChangeRef.current?.(true);
      return () => onSendingChangeRef.current?.(false);
    }
  }, [repeatEnabled, isConnected, isActive]);

  // 键盘 — Shift+Enter 发送，Enter 换行
  const handleKeyDown = useCallback((e: React.KeyboardEvent<HTMLTextAreaElement>) => {
    if (e.key === "Enter" && e.shiftKey) {
      e.preventDefault();
      void doSend();
    }
  }, [doSend]);

  // HEX 过滤
  const handleInputChange = useCallback((e: React.ChangeEvent<HTMLTextAreaElement>) => {
    const val = e.target.value;
    if (sendMode === "hex") {
      dispatch({ type: "SET_INPUT_TEXT", text: val.replace(/[^0-9a-fA-F\s]/g, "") });
    } else {
      dispatch({ type: "SET_INPUT_TEXT", text: val });
    }
  }, [sendMode, dispatch]);

  const handleHistoryClick = useCallback((entry: string) => {
    dispatch({ type: "SET_INPUT_TEXT", text: entry });
    setShowOptions(false);
    inputRef.current?.focus();
  }, [dispatch]);

  // 历史下拉框 — 计算 viewport 固定定位坐标，含边界检测
  const handleToggleHistory = useCallback(() => {
    setShowOptions(prev => {
      if (!prev && historyBtnRef.current) {
        const rect = historyBtnRef.current.getBoundingClientRect();
        const dropdownWidth = 220;
        const maxDropdownH = 180;
        const gap = 4;
        const spaceAbove = rect.top - gap;
        const spaceBelow = window.innerHeight - rect.bottom - gap;
        const openAbove = spaceAbove >= maxDropdownH || spaceAbove >= spaceBelow;
        const availableH = openAbove ? spaceAbove : spaceBelow;
        const maxH = Math.max(0, Math.min(maxDropdownH, availableH));
        const left = Math.max(8, Math.min(rect.right - dropdownWidth, window.innerWidth - dropdownWidth - 8));

        setDropdownStyle({
          position: "fixed",
          left,
          maxHeight: maxH,
          ...(openAbove
            ? { bottom: window.innerHeight - rect.top + gap }
            : { top: rect.bottom + gap }),
        });
      }
      return !prev;
    });
  }, []);

  // 点击下拉框外部时关闭
  useEffect(() => {
    if (!showOptions) return;
    const handleClickOutside = (e: MouseEvent) => {
      if (historyBtnRef.current && !historyBtnRef.current.contains(e.target as Node)) {
        const dropdown = document.querySelector(`.${styles.historyDropdown}`);
        if (dropdown && !dropdown.contains(e.target as Node)) {
          setShowOptions(false);
        }
      }
    };
    document.addEventListener("mousedown", handleClickOutside);
    return () => document.removeEventListener("mousedown", handleClickOutside);
  }, [showOptions]);

  return (
    <div className={styles.basicSend}>
      <div className={styles.inputArea}>
        <textarea
          ref={inputRef}
          className={`${styles.inputField} liquid-glass-input ${sendMode === "hex" ? styles.hexInput : ""}`}
          value={inputText}
          onChange={handleInputChange}
          onKeyDown={handleKeyDown}
          placeholder={
            isConnected
              ? sendMode === "hex" ? "FF 01 02..." : t("sendBar.placeholder")
              : t("sendBar.disconnected")
          }
          disabled={!isConnected}
          rows={3}
          wrap="off"
        />
      </div>

      <div className={styles.controls}>
        <div className={styles.dropdown}>
          <select
            className={`${styles.select} liquid-glass-input liquid-glass-select`}
            value={newlineMode}
            onChange={(e) => dispatch({ type: "SET_NEWLINE_MODE", mode: e.target.value as NewlineMode })}
            title={t("sendBar.appendNewline")}
            disabled={!isConnected || sendMode === "hex"}
          >
            <option value="crlf">{t("sendBar.newline_crlf")}</option>
            <option value="lf">{t("sendBar.newline_lf")}</option>
            <option value="cr">{t("sendBar.newline_cr")}</option>
            <option value="none">{t("sendBar.newline_none")}</option>
          </select>
        </div>

        <button
          className={`${styles.modeBtn} liquid-glass-button liquid-selector-button ${sendMode === "hex" ? "liquid-theme-selected" : ""}`}
          onClick={() => dispatch({ type: "SET_SEND_MODE", mode: sendMode === "text" ? "hex" : "text" })}
          title={t("sendBar.sendMode")}
          type="button"
          aria-pressed={sendMode === "hex"}
          disabled={!isConnected}
        >
          {sendMode === "text" ? t("sendBar.sendModeText") : t("sendBar.sendModeHex")}
        </button>

        <div className={styles.groupSep} />

        <label className="liquid-glass-toggle" title={t("sendBar.repeatSend")}>
          <input
            type="checkbox"
            checked={repeatEnabled}
            onChange={(e) => dispatch({ type: "SET_REPEAT_ENABLED", enabled: e.target.checked })}
            disabled={!isConnected}
          />
          <div />
        </label>

        <div className={styles.intervalWrap}>
          <input
            type="number"
            className={`${styles.intervalInput} liquid-glass-input`}
            value={repeatInterval}
            onChange={(e) => dispatch({ type: "SET_REPEAT_INTERVAL", ms: Math.max(50, Number(e.target.value) || 50) })}
            min={50}
            step={100}
            title={t("sendBar.interval")}
            disabled={!isConnected || !repeatEnabled}
          />
        </div>

        <span className={styles.intervalUnit}>ms</span>

        <div className={styles.groupSep} />

        <div className={styles.historyWrap}>
          <button
            ref={historyBtnRef}
            className={`${styles.historyBtn} liquid-glass-button`}
            onClick={handleToggleHistory}
            title={t("sendBar.sendHistory")}
            disabled={sendHistory.length === 0}
          >
            <Icon name="caret-down" size="xs" />
          </button>
        </div>
        {showOptions && sendHistory.length > 0 && createPortal(
          <div className={`${styles.historyDropdown} liquid-glass-float`} style={dropdownStyle}>
            <div className={styles.historyTitle}>{t("sendBar.sendHistory")}</div>
            <div className={styles.historyList}>
              {sendHistory.slice(0, 20).map((entry, i) => (
                <button
                  key={i}
                  className={styles.historyItem}
                  onClick={() => handleHistoryClick(entry)}
                  title={entry}
                >
                  {entry.length > 40 ? entry.slice(0, 40) + "..." : entry}
                </button>
              ))}
            </div>
          </div>,
          document.body
        )}

        <button
          className={`${styles.sendBtn} liquid-primary-button`}
          onClick={() => { void doSend(); }}
          disabled={!isConnected || (sendMode === "text" && !inputText.trim()) || (sendMode === "hex" && !isHexInputValid(inputText))}
        >
          {t("sendBar.send")}
        </button>
      </div>
    </div>
  );
}
