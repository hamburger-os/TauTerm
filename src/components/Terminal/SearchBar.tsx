import { useState, useCallback, useRef, useEffect, useMemo } from "react";
import { createPortal } from "react-dom";
import { useTranslation } from "react-i18next";
import { motion, AnimatePresence } from "framer-motion";
import type { Terminal as XTerm } from "@xterm/xterm";
import Icon from "../common/Icon";
import styles from "./SearchBar.module.css";

interface SearchBarProps {
  isOpen: boolean;
  onClose: () => void;
  terminal: XTerm | null;
  /** 上次会话保存的搜索关键字（由父组件管理生命周期） */
  savedQuery?: string;
  /** 上次会话保存的大小写标志 */
  savedCaseSensitive?: boolean;
  /** 搜索栏关闭时，父组件保存当前搜索状态 */
  onSaveState?: (query: string, caseSensitive: boolean) => void;
  /** 终端视口 DOM ref — 用于计算 portal 定位坐标 */
  viewportRef?: React.RefObject<HTMLDivElement | null>;
}

interface Match {
  /** 行号（绝对 buffer 行） */
  line: number;
  /** 列起始位置 */
  col: number;
}

/**
 * 终端搜索覆盖层
 *
 * 使用 xterm.js buffer API 扫描终端内容，
 * 实现真实的高亮、导航和滚动到匹配位置。
 */
export default function SearchBar({ isOpen, onClose, terminal, savedQuery = "", savedCaseSensitive = false, onSaveState, viewportRef }: SearchBarProps) {
  const { t } = useTranslation();
  const [query, setQuery] = useState(savedQuery);
  const [caseSensitive, setCaseSensitive] = useState(savedCaseSensitive);
  const [matchIndex, setMatchIndex] = useState(0);
  const inputRef = useRef<HTMLInputElement>(null);
  const markersRef = useRef<ReturnType<XTerm["registerMarker"]>[]>([]);
  const prevOpenRef = useRef(isOpen);

  // 根据 viewportRef 计算固定定位坐标（portal 到 body 后使用 position:fixed）
  const [barPos, setBarPos] = useState<{ top: number; right: number }>({ top: 0, right: 0 });

  const updateBarPos = useCallback(() => {
    if (!viewportRef?.current) return;
    const rect = viewportRef.current.getBoundingClientRect();
    setBarPos({ top: rect.top, right: window.innerWidth - rect.right });
  }, [viewportRef]);

  useEffect(() => {
    if (!isOpen) return;
    updateBarPos();
    let rafId: number | null = null;
    const handleResize = () => {
      if (rafId !== null) cancelAnimationFrame(rafId);
      rafId = requestAnimationFrame(updateBarPos);
    };
    window.addEventListener("resize", handleResize);
    return () => {
      window.removeEventListener("resize", handleResize);
      if (rafId !== null) cancelAnimationFrame(rafId);
    };
  }, [isOpen, updateBarPos]);

  useEffect(() => {
    if (isOpen && !prevOpenRef.current) {
      // 搜索栏刚打开：恢复父组件保存的搜索状态并聚焦
      setQuery(savedQuery);
      setCaseSensitive(savedCaseSensitive);
      setMatchIndex(1);
      inputRef.current?.focus();
    }
    prevOpenRef.current = isOpen;
  }, [isOpen, savedQuery, savedCaseSensitive]);

  // 查找所有匹配位置（仅在搜索栏打开时计算，避免不必要扫描）
  const matches = useMemo(() => {
    if (!isOpen || !query || !terminal) return [] as Match[];
    const results: Match[] = [];
    const buffer = terminal.buffer.active;
    const flags = caseSensitive ? "" : "i";
    const escaped = query.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
    let regex: RegExp;
    try {
      regex = new RegExp(escaped, flags);
    } catch {
      return [];
    }

    for (let row = 0; row < buffer.length; row++) {
      const line = buffer.getLine(row);
      if (!line) continue;
      const text = line.translateToString();
      // 重置 lastIndex 后用 exec 循环匹配
      regex.lastIndex = 0;
      let m: RegExpExecArray | null;
      while ((m = regex.exec(text)) !== null) {
        results.push({ line: row, col: m.index });
        if (m[0].length === 0) break; // 防止零长度匹配死循环
      }
    }
    return results;
  }, [query, caseSensitive, terminal, isOpen]);

  const totalMatches = matches.length;

  // 当前选中匹配
  const currentMatch = totalMatches > 0 ? matches[matchIndex - 1] : null;

  // 更新高亮 markers（在滚动条上显示匹配位置）
  useEffect(() => {
    markersRef.current.forEach(d => d.dispose());
    markersRef.current = [];

    if (!isOpen) {
      try { terminal?.select(0, 0, 0); } catch { /* ignore */ }
      return;
    }

    if (!terminal || totalMatches === 0 || !query) return;

    try {
      const buffer = terminal.buffer.active;
      const viewportTop = buffer.viewportY;
      const viewportBottom = buffer.viewportY + terminal.rows - 1;

      matches.forEach((_m, i) => {
        // 仅注册当前视口内可见行的 marker，避免大缓冲区中注册数千个标记
        if (_m.line < viewportTop || _m.line > viewportBottom) return;

        const isActive = i === matchIndex - 1;
        const marker = terminal.registerMarker(
          -(buffer.baseY + buffer.cursorY - _m.line)
        );
        markersRef.current.push(marker);

        if (isActive && marker) {
          try {
            terminal.select(_m.col, query.length, _m.line - viewportTop);
          } catch {
            // 选择可能因滚动区域外而失败
          }
        }
      });
    } catch {
      // markers API 错误，忽略
    }
  }, [isOpen, matches, matchIndex, terminal, query, totalMatches]);

  // 导航到当前匹配
  useEffect(() => {
    if (!terminal || !currentMatch) return;

    try {
      const buffer = terminal.buffer.active;
      // 计算行在视口中的位置或滚动
      const absoluteLine = currentMatch.line;
      const viewportTop = buffer.baseY;
      const viewportBottom = buffer.baseY + terminal.rows - 1;

      if (absoluteLine < viewportTop || absoluteLine > viewportBottom) {
        // 需要滚动：将目标行放在视口中间
        terminal.scrollToLine(absoluteLine - Math.floor(terminal.rows / 2));
      }

      // 选中匹配文本以高亮显示
      const queryLen = query.length;
      const viewportLine = absoluteLine - buffer.viewportY;
      if (viewportLine >= 0 && viewportLine < terminal.rows) {
        terminal.select(currentMatch.col, queryLen, viewportLine);
      }
    } catch {
      // 忽略选择错误
    }
  }, [currentMatch, terminal, query]);

  // 关闭时清理
  useEffect(() => {
    return () => {
      markersRef.current.forEach(d => d.dispose());
      markersRef.current = [];
      try { terminal?.select(0, 0, 0); } catch { /* ignore */ }
    };
  }, [terminal, isOpen]);

  const handleKeyDown = useCallback((e: React.KeyboardEvent) => {
    if (e.key === "Escape") {
      onSaveState?.(query, caseSensitive);
      onClose();
    } else if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "f") {
      e.preventDefault();
      onSaveState?.(query, caseSensitive);
      onClose();
    } else if (e.key === "Enter") {
      e.preventDefault();
      if (totalMatches === 0) return;
      if (e.shiftKey) {
        // 上一个匹配
        setMatchIndex(prev => (prev > 1 ? prev - 1 : totalMatches));
      } else {
        // 下一个匹配
        setMatchIndex(prev => (prev < totalMatches ? prev + 1 : 1));
      }
    }
  }, [onClose, onSaveState, query, caseSensitive, totalMatches]);

  const handleClose = useCallback(() => {
    onSaveState?.(query, caseSensitive);
    onClose();
  }, [onClose, onSaveState, query, caseSensitive]);

  return createPortal(
    <AnimatePresence>
      {isOpen && (
        <motion.div
          key="search-bar"
          className={`${styles.bar} liquid-glass-float`}
          style={{ position: "fixed", top: barPos.top, right: barPos.right, zIndex: 100 }}
          initial={{ opacity: 0, y: -10 }}
          animate={{ opacity: 1, y: 0 }}
          exit={{ opacity: 0, y: -10 }}
        >
          <input
            ref={inputRef}
            className={`${styles.input} liquid-glass-input`}
            type="text"
            placeholder={t("search.terminalPlaceholder") || "Find in terminal..."}
            value={query}
            onChange={(e) => { setQuery(e.target.value); setMatchIndex(1); }}
            onKeyDown={handleKeyDown}
          />
          <span className={styles.count}>
            {totalMatches > 0 ? `${matchIndex}/${totalMatches}` : query ? "0/0" : ""}
          </span>
          <button
            className={`${styles.btn} liquid-glass-button ${caseSensitive ? styles.active : ""}`}
            onClick={() => setCaseSensitive(!caseSensitive)}
            title="Case sensitive"
          >
            Aa
          </button>
          <button className={`${styles.btn} liquid-glass-button`} onClick={() => setMatchIndex(prev => prev > 1 ? prev - 1 : totalMatches)}>
            <Icon name="chevron-up" size="sm" />
          </button>
          <button className={`${styles.btn} liquid-glass-button`} onClick={() => setMatchIndex(prev => prev < totalMatches ? prev + 1 : 1)}>
            <Icon name="chevron-down" size="sm" />
          </button>
          <button className={`${styles.btn} liquid-glass-button`} onClick={handleClose}><Icon name="close" size="sm" /></button>
        </motion.div>
      )}
    </AnimatePresence>,
    document.body
  );
}
