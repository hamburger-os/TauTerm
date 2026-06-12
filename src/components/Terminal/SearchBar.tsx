import { useState, useCallback, useRef, useEffect } from "react";
import { useTranslation } from "react-i18next";
import { motion, AnimatePresence } from "framer-motion";
import styles from "./SearchBar.module.css";

interface SearchBarProps {
  onClose: () => void;
}

/**
 * 终端搜索覆盖层
 *
 * 使用 xterm.js 内置搜索能力或手动扫描 buffer。
 * 当前实现使用 DOM 文本搜索作为基础方案。
 */
export default function SearchBar({ onClose }: SearchBarProps) {
  const { t } = useTranslation();
  const [query, setQuery] = useState("");
  const [caseSensitive, setCaseSensitive] = useState(false);
  const [matchIndex, setMatchIndex] = useState(0);
  const [totalMatches, setTotalMatches] = useState(0);
  const inputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    inputRef.current?.focus();
  }, []);

  useEffect(() => {
    // Search terminal content
    if (!query) {
      setTotalMatches(0);
      setMatchIndex(0);
      return;
    }

    const terminalEl = document.querySelector(".xterm-screen");
    if (!terminalEl) return;

    const text = terminalEl.textContent || "";
    const flags = caseSensitive ? "g" : "gi";
    const regex = new RegExp(query.replace(/[.*+?^${}()|[\]\\]/g, "\\$&"), flags);
    const matches = text.match(regex);
    setTotalMatches(matches ? matches.length : 0);
    setMatchIndex(matches ? 1 : 0);
  }, [query, caseSensitive]);

  const handleKeyDown = useCallback((e: React.KeyboardEvent) => {
    if (e.key === "Escape") {
      onClose();
    } else if (e.key === "Enter") {
      if (e.shiftKey) {
        // Previous match
        setMatchIndex(prev => (prev > 1 ? prev - 1 : totalMatches));
      } else {
        // Next match
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
          className={styles.input}
          type="text"
          placeholder={t("search.placeholder") || "Search..."}
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          onKeyDown={handleKeyDown}
        />
        <span className={styles.count}>
          {totalMatches > 0 ? `${matchIndex}/${totalMatches}` : query ? "0/0" : ""}
        </span>
        <button
          className={`${styles.btn} ${caseSensitive ? styles.active : ""}`}
          onClick={() => setCaseSensitive(!caseSensitive)}
          title="Case sensitive"
        >
          Aa
        </button>
        <button className={styles.btn} onClick={() => setMatchIndex(prev => prev > 1 ? prev - 1 : totalMatches)}>
          ↑
        </button>
        <button className={styles.btn} onClick={() => setMatchIndex(prev => prev < totalMatches ? prev + 1 : 1)}>
          ↓
        </button>
        <button className={styles.btn} onClick={onClose}>×</button>
      </motion.div>
    </AnimatePresence>
  );
}
