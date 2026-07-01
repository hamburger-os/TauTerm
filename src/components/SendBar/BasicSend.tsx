import { useState, useCallback, useEffect, useRef } from "react";
import { useTranslation } from "react-i18next";
import { useSession } from "../../context/SessionContext";
import Icon from "../common/Icon";
import type { NewlineMode, SendMode } from "./types";
import styles from "./BasicSend.module.css";

interface BasicSendProps {
  sessionId: string;
  onSendingChange?: (sending: boolean) => void;
}

const NEWLINE_MAP: Record<NewlineMode, string> = {
  crlf: "\r\n",
  lf: "\n",
  cr: "\r",
  none: "",
};

/**
 * 基础发送面板
 *
 * 支持文本/HEX 输入、换行符追加、重复发送、发送历史。
 * 从原 SendBar.tsx 提取，逻辑保持不变。
 */
export default function BasicSend({ sessionId, onSendingChange }: BasicSendProps) {
  const { t } = useTranslation();
  const { sendData, state } = useSession();
  const activeTab = state.tabs.find(tab => tab.id === sessionId);

  const [inputText, setInputText] = useState("");
  const [newlineMode, setNewlineMode] = useState<NewlineMode>("crlf");
  const [sendMode, setSendMode] = useState<SendMode>(() => {
    const stored = localStorage.getItem("tauterm-default-data-mode");
    return stored === "hex" ? "hex" : "text";
  });
  const [repeatEnabled, setRepeatEnabled] = useState(false);
  const [repeatInterval, setRepeatInterval] = useState(1000);
  const [sendHistory, setSendHistory] = useState<string[]>([]);
  const [showOptions, setShowOptions] = useState(false);

  const inputRef = useRef<HTMLTextAreaElement>(null);
  const intervalRef = useRef<ReturnType<typeof setInterval> | null>(null);
  const inputRefForInterval = useRef(inputText);
  inputRefForInterval.current = inputText;

  const isConnected = activeTab?.state === "connected" || activeTab?.state === "transferring";

  const isHexValid = (value: string): boolean => {
    const hex = value.replace(/\s/g, "");
    return hex.length > 0 && hex.length % 2 === 0 && /^[0-9a-fA-F]+$/.test(hex);
  };

  const doSend = useCallback(() => {
    const currentInput = inputRefForInterval.current;
    if (!currentInput.trim() && sendMode === "text") return;

    let data: string | Uint8Array;
    if (sendMode === "hex") {
      const hex = currentInput.replace(/\s/g, "");
      if (hex.length === 0 || hex.length % 2 !== 0) return;
      if (!/^[0-9a-fA-F]+$/.test(hex)) return;
      const len = hex.length / 2;
      const bytes = new Uint8Array(len);
      for (let i = 0; i < len; i++) {
        bytes[i] = parseInt(hex.substring(i * 2, i * 2 + 2), 16);
      }
      data = bytes;
    } else {
      data = currentInput + NEWLINE_MAP[newlineMode];
    }

    sendData(sessionId, data);

    setSendHistory(prev => {
      const entry = currentInput;
      const next = [entry, ...prev.filter(h => h !== entry)];
      return next.slice(0, 50);
    });

    inputRef.current?.focus();
  }, [newlineMode, sendMode, sessionId, sendData]);

  const doIntervalSend = useCallback(() => {
    const currentInput = inputRefForInterval.current;
    if (!currentInput.trim() && sendMode === "text") return;

    let data: string | Uint8Array;
    if (sendMode === "hex") {
      const hex = currentInput.replace(/\s/g, "");
      if (hex.length === 0 || hex.length % 2 !== 0) return;
      if (!/^[0-9a-fA-F]+$/.test(hex)) return;
      const len = hex.length / 2;
      const bytes = new Uint8Array(len);
      for (let i = 0; i < len; i++) {
        bytes[i] = parseInt(hex.substring(i * 2, i * 2 + 2), 16);
      }
      data = bytes;
    } else {
      data = currentInput + NEWLINE_MAP[newlineMode];
    }

    sendData(sessionId, data);

    setSendHistory(prev => {
      const entry = currentInput;
      const next = [entry, ...prev.filter(h => h !== entry)];
      return next.slice(0, 50);
    });
  }, [newlineMode, sendMode, sessionId, sendData]);

  // 重复发送定时器
  useEffect(() => {
    if (intervalRef.current) {
      clearInterval(intervalRef.current);
      intervalRef.current = null;
    }
    if (!isConnected) return;
    const hasValidInput = sendMode === "hex"
      ? isHexValid(inputRefForInterval.current)
      : inputRefForInterval.current.trim().length > 0;
    if (repeatEnabled && repeatInterval >= 50 && hasValidInput) {
      intervalRef.current = setInterval(doIntervalSend, repeatInterval);
    }
    return () => {
      if (intervalRef.current) clearInterval(intervalRef.current);
    };
  }, [isConnected, repeatEnabled, repeatInterval, sendMode]);

  // 断开会话时重置
  const prevConnectedRef = useRef(isConnected);
  useEffect(() => {
    const wasConnected = prevConnectedRef.current;
    prevConnectedRef.current = isConnected;
    if (wasConnected && !isConnected) {
      setRepeatEnabled(false);
      setRepeatInterval(1000);
      setInputText("");
      setSendHistory([]);
      setShowOptions(false);
    }
  }, [isConnected]);

  // 通知父组件重复发送状态（用于锁定模式切换）
  const onSendingChangeRef = useRef(onSendingChange);
  onSendingChangeRef.current = onSendingChange;
  useEffect(() => {
    if (repeatEnabled && isConnected) {
      onSendingChangeRef.current?.(true);
      return () => onSendingChangeRef.current?.(false);
    }
  }, [repeatEnabled, isConnected]);

  // 键盘 — Shift+Enter 发送，Enter 换行
  const handleKeyDown = useCallback((e: React.KeyboardEvent<HTMLTextAreaElement>) => {
    if (e.key === "Enter" && e.shiftKey) {
      e.preventDefault();
      doSend();
    }
  }, [doSend]);

  // HEX 过滤
  const handleInputChange = useCallback((e: React.ChangeEvent<HTMLTextAreaElement>) => {
    const val = e.target.value;
    if (sendMode === "hex") {
      const filtered = val.replace(/[^0-9a-fA-F\s]/g, "");
      setInputText(filtered);
    } else {
      setInputText(val);
    }
  }, [sendMode]);

  const handleHistoryClick = useCallback((entry: string) => {
    setInputText(entry);
    setShowOptions(false);
    inputRef.current?.focus();
  }, []);

  return (
    <div className={styles.basicSend}>
      {/* 输入区域 */}
      <div className={styles.inputArea}>
        <textarea
          ref={inputRef}
          className={`${styles.inputField} liquid-glass-input ${sendMode === "hex" ? styles.hexInput : ""}`}
          value={inputText}
          onChange={handleInputChange}
          onKeyDown={handleKeyDown}
          placeholder={
            isConnected
              ? sendMode === "hex" ? "FF 01 02..." : (t("sendBar.placeholder") || "Type data to send...")
              : t("sendBar.disconnected")
          }
          disabled={!isConnected}
          rows={3}
          wrap="off"
        />
      </div>

      {/* 3 行控件区 */}
      <div className={styles.controls}>
        {/* Row 1: 换行符选择 + 发送模式切换 */}
        <div className={styles.controlsRow1}>
          <div className={styles.dropdown}>
            <select
              className={`${styles.select} liquid-glass-input`}
              value={newlineMode}
              onChange={(e) => setNewlineMode(e.target.value as NewlineMode)}
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
            className={`${styles.modeBtn} liquid-glass-button ${sendMode === "hex" ? styles.modeActive : ""}`}
            onClick={() => setSendMode(m => m === "text" ? "hex" : "text")}
            title={t("sendBar.sendMode")}
            disabled={!isConnected}
          >
            {sendMode === "text" ? t("sendBar.sendModeText") : t("sendBar.sendModeHex")}
          </button>
        </div>

        {/* Row 2: 重复发送间隔 + 开关 */}
        <div className={styles.controlsRow2}>
          <input
            type="number"
            className={`${styles.intervalInput} liquid-glass-input`}
            value={repeatInterval}
            onChange={(e) => setRepeatInterval(Math.max(50, Number(e.target.value)))}
            min={50}
            step={100}
            title={t("sendBar.interval")}
            disabled={!isConnected || !repeatEnabled}
          />
          <label className={styles.repeatLabel} title={t("sendBar.repeatSend")}>
            <input
              type="checkbox"
              className={styles.repeatCheck}
              checked={repeatEnabled}
              onChange={(e) => setRepeatEnabled(e.target.checked)}
              disabled={!isConnected}
            />
            <div className={styles.toggleTrack} />
            <Icon name="loop" size="xs" />
          </label>
        </div>

        {/* Row 3: 发送历史 + 发送按钮 */}
        <div className={styles.controlsRow3}>
          <div className={styles.historyWrap}>
            <button
              className={`${styles.historyBtn} liquid-glass-button`}
              onClick={() => setShowOptions(o => !o)}
              title={t("sendBar.sendHistory")}
              disabled={sendHistory.length === 0}
            >
              <Icon name="chevron-dropdown" size="xs" />
            </button>
            {showOptions && sendHistory.length > 0 && (
              <div className={styles.historyDropdown}>
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
              </div>
            )}
          </div>
          <button
            className={`${styles.sendBtn} liquid-primary-button`}
            onClick={doSend}
            disabled={!isConnected || (sendMode === "text" && !inputText.trim()) || (sendMode === "hex" && !isHexValid(inputText))}
          >
            {t("sendBar.send")}
          </button>
        </div>
      </div>
    </div>
  );
}
