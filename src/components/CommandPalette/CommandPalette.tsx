import { useState, useEffect, useCallback, useRef, useMemo } from "react";
import { useTranslation } from "react-i18next";
import { motion, AnimatePresence } from "framer-motion";
import { shortcutRegistry } from "../../shortcuts/registry";
import type { ShortcutActionId } from "../../shortcuts/actionIds";
import styles from "./CommandPalette.module.css";

interface CommandItem {
  id: ShortcutActionId;
  label: string;
  category: string;
  shortcut?: string;
}

interface CommandPaletteProps {
  isOpen: boolean;
  onClose: () => void;
  onExecute: (commandId: ShortcutActionId) => void;
}

/**
 * 命令面板
 *
 * Ctrl+Shift+P 打开，支持模糊搜索所有命令。
 */
export default function CommandPalette({ isOpen, onClose, onExecute }: CommandPaletteProps) {
  const { t } = useTranslation();
  const [query, setQuery] = useState("");
  const [selectedIndex, setSelectedIndex] = useState(0);
  const inputRef = useRef<HTMLInputElement>(null);

  // 构建命令列表
  const allCommands: CommandItem[] = useMemo(() => {
    const shortcuts = shortcutRegistry.getAll();
    return shortcuts.map(s => ({
      id: s.id,
      label: s.description,
      category: s.category,
      shortcut: s.keys,
    }));
  }, []);

  // 模糊搜索
  const filtered = useMemo(() => {
    if (!query.trim()) return allCommands;
    const q = query.toLowerCase();
    return allCommands.filter(c =>
      c.label.toLowerCase().includes(q) ||
      c.id.toLowerCase().includes(q) ||
      c.category.toLowerCase().includes(q)
    );
  }, [query, allCommands]);

  // 分组
  const grouped = useMemo(() => {
    const map = new Map<string, CommandItem[]>();
    for (const cmd of filtered) {
      const list = map.get(cmd.category) || [];
      list.push(cmd);
      map.set(cmd.category, list);
    }
    return map;
  }, [filtered]);

  useEffect(() => {
    if (isOpen) {
      setQuery("");
      setSelectedIndex(0);
      setTimeout(() => inputRef.current?.focus(), 50);
    }
  }, [isOpen]);

  // 选中范围内平铺列表 index
  const flatList = filtered;

  // 预计算索引 Map，避免 O(n²) 的 indexOf 查找
  const flatIndex = useMemo(() => {
    const map = new Map<string, number>();
    flatList.forEach((cmd, i) => map.set(cmd.id, i));
    return map;
  }, [flatList]);

  const handleSelect = useCallback((command: CommandItem) => {
    onExecute(command.id);
    onClose();
  }, [onExecute, onClose]);

  const handleKeyDown = useCallback((e: React.KeyboardEvent) => {
    if (e.key === "Escape") {
      onClose();
    } else if (e.key === "ArrowDown") {
      e.preventDefault();
      setSelectedIndex(prev => Math.min(prev + 1, flatList.length - 1));
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      setSelectedIndex(prev => Math.max(prev - 1, 0));
    } else if (e.key === "Enter") {
      e.preventDefault();
      if (flatList[selectedIndex]) {
        handleSelect(flatList[selectedIndex]);
      }
    }
  }, [onClose, flatList, selectedIndex, handleSelect]);

  return (
    <AnimatePresence>
      {isOpen && (
        <motion.div
          className={`${styles.overlay} glass-overlay`}
          initial={{ opacity: 0 }}
          animate={{ opacity: 1 }}
          exit={{ opacity: 0 }}
          transition={{ duration: 0.12, ease: [0.4, 0, 0.2, 1] }}
          onClick={onClose}
        >
          <motion.div
            initial={{ y: -20, scale: 0.95, opacity: 0 }}
            animate={{ y: 0, scale: 1, opacity: 1 }}
            exit={{ y: -20, scale: 0.95, opacity: 0 }}
            transition={{ duration: 0.11, delay: 0.04, ease: [0.4, 0, 0.2, 1] }}
            onClick={(e) => e.stopPropagation()}
          >
            <div className={`${styles.palette} liquid-glass`}>
          <input
            ref={inputRef}
            className={styles.input}
            type="text"
            placeholder={t("palette.placeholder") || "Type a command..."}
            value={query}
            onChange={(e) => { setQuery(e.target.value); setSelectedIndex(0); }}
            onKeyDown={handleKeyDown}
          />
          <div className={styles.list}>
            {flatList.length === 0 ? (
              <div className={styles.empty}>{t("palette.noResults") || "No commands found"}</div>
            ) : (
              Array.from(grouped.entries()).map(([category, commands]) => (
                <div key={category} className={styles.group}>
                  <div className={styles.groupTitle}>{category}</div>
                  {commands.map(cmd => {
                    const idx = flatIndex.get(cmd.id) ?? 0;
                    return (
                      <div
                        key={cmd.id}
                        className={`${styles.item} ${idx === selectedIndex ? styles.selected : ""}`}
                        onClick={() => handleSelect(cmd)}
                        onMouseEnter={() => setSelectedIndex(idx)}
                      >
                        <span className={styles.itemLabel}>{cmd.label}</span>
                        {cmd.shortcut && <span className={styles.itemShortcut}>{cmd.shortcut}</span>}
                      </div>
                    );
                  })}
                </div>
              ))
            )}
          </div>
          </div>
        </motion.div>
        </motion.div>
      )}
    </AnimatePresence>
  );
}
