import { useState, useCallback, useRef, useEffect, useMemo } from "react";
import { useTranslation } from "react-i18next";
import { motion, AnimatePresence } from "framer-motion";
import type { Terminal as XTerm } from "@xterm/xterm";
import styles from "./SearchBar.module.css";

interface SearchBarProps {
  onClose: () => void;
  terminal: XTerm | null;
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
export default function SearchBar({ onClose, terminal }: SearchBarProps) {
  const { t } = useTranslation();
  const [query, setQuery] = useState("");
  const [caseSensitive, setCaseSensitive] = useState(false);
  const [matchIndex, setMatchIndex] = useState(0);
  const inputRef = useRef<HTMLInputElement>(null);
  const markersRef = useRef<ReturnType<XTerm["registerMarker"]>[]>([]);

  useEffect(() => {
    inputRef.current?.focus();
  }, []);

  // 查找所有匹配位置
  const matches = useMemo(() => {
    if (!query || !terminal) return [] as Match[];
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
  }, [query, caseSensitive, terminal]);

  const totalMatches = matches.length;

  // 当前选中匹配
  const currentMatch = totalMatches > 0 ? matches[matchIndex - 1] : null;

  // 更新高亮 markers（在滚动条上显示匹配位置）
  useEffect(() => {
    markersRef.current.forEach(d => d.dispose());
    markersRef.current = [];

    if (!terminal || totalMatches === 0 || !query) return;

    try {
      matches.forEach((_m, i) => {
        const isActive = i === matchIndex - 1;
        const marker = terminal.registerMarker(
          -(terminal.buffer.active.baseY + terminal.buffer.active.cursorY - _m.line)
        );
        markersRef.current.push(marker);

        if (isActive && marker) {
          try {
            terminal.select(_m.col, query.length, _m.line - terminal.buffer.active.viewportY);
          } catch {
            // 选择可能因滚动区域外而失败
          }
        }
      });
    } catch {
      // markers API 错误，忽略
    }
  }, [matches, matchIndex, terminal, query, totalMatches]);

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
  }, [terminal]);

  const handleKeyDown = useCallback((e: React.KeyboardEvent) => {
    if (e.key === "Escape") {
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
  }, [onClose, totalMatches]);

  return (
    <AnimatePresence>
      <motion.div
        className={styles.bar}
        initial={{ opacity: 0, y: -10 }}
        animate={{ opacity: 1, y: 0 }}
        exit={{ opacity: 0, y: -10 }}
      >
        <input
          ref={inputRef}
          className={`${styles.input} liquid-glass-input ${query && totalMatches === 0 ? styles.noMatch : ""}`}
          type="text"
          placeholder={t("search.placeholder") || "Search..."}
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
          ↑
        </button>
        <button className={`${styles.btn} liquid-glass-button`} onClick={() => setMatchIndex(prev => prev < totalMatches ? prev + 1 : 1)}>
          ↓
        </button>
        <button className={`${styles.btn} liquid-glass-button`} onClick={onClose}>×</button>
      </motion.div>
    </AnimatePresence>
  );
}
