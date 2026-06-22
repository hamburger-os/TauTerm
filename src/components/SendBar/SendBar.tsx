import { useState, useCallback, useEffect, useRef } from "react";
import { useTranslation } from "react-i18next";
import { useSession } from "../../context/SessionContext";
import styles from "./SendBar.module.css";

interface SendBarProps {
  sessionId: string;
}

type NewlineMode = "crlf" | "lf" | "cr" | "none";
type SendMode = "text" | "hex";

const NEWLINE_MAP: Record<NewlineMode, string> = {
  crlf: "\r\n",
  lf: "\n",
  cr: "\r",
  none: "",
};

/**
 * 发送栏组件
 *
 * 位于主内容区底部，支持文本/HEX 输入、换行符追加、
 * 重复发送、发送历史等功能。
 */
export default function SendBar({ sessionId }: SendBarProps) {
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
  // 同步 ref 以便定时器回调读取最新值
  inputRefForInterval.current = inputText;

  const isConnected = activeTab?.state === "connected" || activeTab?.state === "transferring";

  // HEX 输入有效性检查：非空、偶数长度、纯十六进制字符
  const isHexValid = (value: string): boolean => {
    const hex = value.replace(/\s/g, "");
    return hex.length > 0 && hex.length % 2 === 0 && /^[0-9a-fA-F]+$/.test(hex);
  };

  // 发送逻辑
  const doSend = useCallback(() => {
    // 使用 ref 读取最新值，避免定时器回调因闭包陈旧而发送过时数据
    const currentInput = inputRefForInterval.current;
    if (!currentInput.trim() && sendMode === "text") return;

    let data: string | Uint8Array;
    if (sendMode === "hex") {
      const hex = currentInput.replace(/\s/g, "");
      // 必须为非空且长度为偶数
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

    // 添加到发送历史 — 存储原始输入（不含换行符追加），避免换行符翻倍
    setSendHistory(prev => {
      const entry = currentInput;
      const next = [entry, ...prev.filter(h => h !== entry)];
      return next.slice(0, 50);
    });

    // 保持输入内容不清空，方便重复发送
    inputRef.current?.focus();
  }, [newlineMode, sendMode, sessionId, sendData]);

  // 重复发送定时器 — doSend 通过 ref 读取输入值，不受 doSend 引用变化影响
  // 只依赖开关和间隔变化，避免因 newlineMode/sendMode 切换导致不必要的定时器重建
  useEffect(() => {
    if (intervalRef.current) {
      clearInterval(intervalRef.current);
      intervalRef.current = null;
    }
    const hasValidInput = sendMode === "hex"
      ? isHexValid(inputRefForInterval.current)
      : inputRefForInterval.current.trim().length > 0;
    if (repeatEnabled && repeatInterval >= 50 && hasValidInput) {
      intervalRef.current = setInterval(doSend, repeatInterval);
    }
    return () => {
      if (intervalRef.current) clearInterval(intervalRef.current);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [repeatEnabled, repeatInterval]);

  // 键盘处理
  const handleKeyDown = useCallback((e: React.KeyboardEvent<HTMLTextAreaElement>) => {
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      doSend();
    }
  }, [doSend]);

  // HEX 输入过滤
  const handleInputChange = useCallback((e: React.ChangeEvent<HTMLTextAreaElement>) => {
    const val = e.target.value;
    if (sendMode === "hex") {
      // 只允许十六进制字符和空格
      const filtered = val.replace(/[^0-9a-fA-F\s]/g, "");
      setInputText(filtered);
    } else {
      setInputText(val);
    }
  }, [sendMode]);

  // 历史项点击
  const handleHistoryClick = useCallback((entry: string) => {
    // 如果是 hex 模式且 entry 包含换行，处理一下
    setInputText(entry);
    setShowOptions(false);
    inputRef.current?.focus();
  }, []);

  return (
    <div className={`${styles.sendBar} liquid-glass`}>
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
          rows={1}
          wrap="off"
        />
      </div>

      {/* 操作按钮区 */}
      <div className={styles.actions}>
        {/* 换行符选择 */}
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

        {/* 发送模式切换 */}
        <button
          className={`${styles.modeBtn} liquid-glass-button ${sendMode === "hex" ? styles.modeActive : ""}`}
          onClick={() => setSendMode(m => m === "text" ? "hex" : "text")}
          title={t("sendBar.sendMode")}
          disabled={!isConnected}
        >
          {sendMode === "text" ? t("sendBar.sendModeText") : t("sendBar.sendModeHex")}
        </button>

        {/* 重复发送 — 液态玻璃切换开关 */}
        <label className={styles.repeatLabel} title={t("sendBar.repeatSend")}>
          <input
            type="checkbox"
            className={styles.repeatCheck}
            checked={repeatEnabled}
            onChange={(e) => setRepeatEnabled(e.target.checked)}
            disabled={!isConnected}
          />
          <div className={styles.toggleTrack} />
          <span className={styles.repeatText}>⟳</span>
        </label>
        {repeatEnabled && (
          <input
            type="number"
            className={`${styles.intervalInput} liquid-glass-input`}
            value={repeatInterval}
            onChange={(e) => setRepeatInterval(Math.max(50, Number(e.target.value)))}
            min={50}
            step={100}
            title={t("sendBar.interval")}
            disabled={!isConnected}
          />
        )}

        {/* 发送历史 */}
        {sendHistory.length > 0 && (
          <div className={styles.historyWrap}>
            <button
              className={`${styles.historyBtn} liquid-glass-button`}
              onClick={() => setShowOptions(o => !o)}
              title={t("sendBar.sendHistory")}
            >
              ▾
            </button>
            {showOptions && (
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
        )}

        {/* 发送按钮 — 炫彩流光 */}
        <button
          className={`${styles.sendBtn} liquid-primary-button`}
          onClick={doSend}
          disabled={!isConnected || (sendMode === "text" && !inputText.trim()) || (sendMode === "hex" && !isHexValid(inputText))}
        >
          {t("sendBar.send")}
        </button>
      </div>
    </div>
  );
}
