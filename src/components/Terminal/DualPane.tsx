import { useRef, useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import styles from "./DualPane.module.css";
import ScrollToBottomButton from "./ScrollToBottomButton";

// ── 数据类型 ──

export interface DualLine {
  /** 单调递增行号，用作 React key */
  id: number;
  /** 毫秒时间戳 [HH:MM:SS.mmm] */
  timestamp: string;
  /** 数据方向 */
  direction: "RX" | "TX";
  /** ASCII 文本（不可打印字符显示为 .） */
  text: string;
  /** HEX 字符串（大写，每 8 字节一组空格分隔） */
  hex: string;
}

interface DualPaneProps {
  lines: DualLine[];
  /** 字体大小 px */
  fontSize?: number;
  /** 行缓冲上限（行数），超限裁剪由父组件 TerminalView 的 flushDualLines 处理 */
  bufferLines?: number;
}

// ── 常量 ──

const DEFAULT_SPLIT = 33.33; // 默认 1:2（ASCII:HEX）
const MIN_PANEL_PCT = 20;  // 最小 20%
const MAX_PANEL_PCT = 80;  // 最大 80%

/**
 * 双栏数据显示面板
 *
 * 采用单滚动容器 + 每帧一行 flex 布局：
 * - 左侧 ASCII 和右侧 HEX 在同一行内各自自动换行
 * - 行高取两者最大值，天然保证左右行对齐
 * - 绝对定位分隔条可拖拽调整列宽
 * - 自动跟踪底部新数据，用户手动上滚后暂停跟踪
 */
export default function DualPane({ lines, fontSize = 13 }: DualPaneProps) {
  const { t } = useTranslation();
  const containerRef = useRef<HTMLDivElement>(null);
  const scrollRef = useRef<HTMLDivElement>(null);
  const dividerRef = useRef<HTMLDivElement>(null);
  /** 拖拽分隔条位置（百分比），使用 state 确保 re-render 时位置不丢失 */
  const [splitPct, setSplitPct] = useState(DEFAULT_SPLIT);
  const [dragging, setDragging] = useState(false);
  const autoScrollRef = useRef(true);
  const [isAtBottom, setIsAtBottom] = useState(true);

  // ── 滚动事件：检测用户是否手动滚离底部 ──

  const handleScroll = useCallback(() => {
    const el = scrollRef.current;
    if (!el) return;
    const atBottom = el.scrollHeight - el.scrollTop - el.clientHeight < 30;
    autoScrollRef.current = atBottom;
    setIsAtBottom(atBottom);
  }, []);

  // ── 新数据自动滚底 ──

  useEffect(() => {
    if (!autoScrollRef.current) return;
    const el = scrollRef.current;
    if (!el) return;
    el.scrollTop = el.scrollHeight;
    // 与 scrollToBottom 的 rAF 模式保持一致：设置 scrollTop 会触发 scroll 事件，
    // 该事件可能在布局稳定前触发，导致错误地计算出非底部位置。
    // 延迟到下一帧重新确认状态。
    requestAnimationFrame(() => {
      autoScrollRef.current = true;
      setIsAtBottom(true);
    });
  }, [lines]);

  // ── 回到底部 ──

  const scrollToBottom = useCallback(() => {
    const el = scrollRef.current;
    if (el) {
      el.scrollTop = el.scrollHeight;
    }
    // 使用 rAF 延迟设置，确保 scroll 事件触发 handleScroll 后再覆盖 autoScrollRef
    // 避免浏览器在 scroll 位置未稳定时触发事件导致 autoScrollRef 被错误重置为 false
    requestAnimationFrame(() => {
      autoScrollRef.current = true;
      setIsAtBottom(true);
    });
  }, []);

  // ── 拖拽分隔条：通过 CSS 变量 --dual-split 控制列宽 ──
  // 使用 useEffect + dragging state 管理 document 级监听器，
  // 确保组件卸载时自动清理（防止监听器泄漏）。

  const handleDividerDown = useCallback((e: React.MouseEvent) => {
    e.preventDefault();
    setDragging(true);
  }, []);

  /** 键盘调整分隔条位置（ArrowLeft/ArrowRight，步长 5%） */
  const handleDividerKey = useCallback((e: React.KeyboardEvent) => {
    if (e.key === "ArrowLeft") {
      setSplitPct(prev => Math.max(MIN_PANEL_PCT, prev - 5));
    } else if (e.key === "ArrowRight") {
      setSplitPct(prev => Math.min(MAX_PANEL_PCT, prev + 5));
    }
  }, []);

  useEffect(() => {
    if (!dragging) return;

    const onMove = (ev: MouseEvent) => {
      if (!containerRef.current) return;
      const rect = containerRef.current.getBoundingClientRect();
      let pct = ((ev.clientX - rect.left) / rect.width) * 100;
      pct = Math.max(MIN_PANEL_PCT, Math.min(MAX_PANEL_PCT, pct));
      setSplitPct(pct);
    };

    const onUp = () => {
      setDragging(false);
    };

    document.addEventListener("mousemove", onMove);
    document.addEventListener("mouseup", onUp);

    return () => {
      document.removeEventListener("mousemove", onMove);
      document.removeEventListener("mouseup", onUp);
    };
  }, [dragging]);

  // ── 渲染 ──

  return (
    <div
      className={styles.container}
      ref={containerRef}
      style={{ fontSize: `${fontSize}px` }}
    >
      {/* 统一滚动区域 */}
      <div
        className={styles.scrollArea}
        ref={scrollRef}
        onScroll={handleScroll}
      >
        {lines.map((line) => (
          <div
            key={line.id}
            className={`${styles.row} ${line.direction === "TX" ? styles.txRow : styles.rxRow}`}
          >
            {/* 左单元格：ASCII 文本 */}
            <div className={styles.asciiCell} style={{ width: `${splitPct}%` }}>
              <span className={styles.dirTag}>[{line.direction}]</span>
              <span className={styles.tsTag}>[{line.timestamp}]</span>
              {line.text}
            </div>

            {/* 右单元格：HEX */}
            <div className={styles.hexCell}>
              {line.hex}
            </div>
          </div>
        ))}
      </div>

      {/* 绝对定位拖拽分隔条（支持鼠标拖拽 + 键盘调整） */}
      <div
        ref={dividerRef}
        className={`${styles.divider} ${dragging ? styles.dividerActive : ""}`}
        style={{ left: `${splitPct}%` }}
        role="separator"
        aria-orientation="vertical"
        aria-valuenow={Math.round(splitPct)}
        aria-valuemin={MIN_PANEL_PCT}
        aria-valuemax={MAX_PANEL_PCT}
        aria-label={t("dualPane.resizeLabel")}
        tabIndex={0}
        onMouseDown={handleDividerDown}
        onKeyDown={handleDividerKey}
      />

      {/* 浮动"回到底部"按钮：用户上滚离开底部时显示 */}
      <ScrollToBottomButton
        visible={!isAtBottom}
        onClick={scrollToBottom}
      />
    </div>
  );
}
