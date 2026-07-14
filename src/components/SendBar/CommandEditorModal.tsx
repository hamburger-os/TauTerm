import { useState, useEffect, useCallback } from "react";
import { createPortal } from "react-dom";
import { motion, AnimatePresence } from "framer-motion";
import { useTranslation } from "react-i18next";
import type { CommandItem } from "./types";
import styles from "./CommandEditorModal.module.css";

interface CommandEditorModalProps {
  isOpen: boolean;
  editItem: CommandItem | null; // null = 新增, non-null = 编辑
  defaultDelay: number;
  onSave: (item: CommandItem) => void;
  onClose: () => void;
}

/**
 * 命令编辑弹窗
 *
 * 用于新增或编辑单条命令，包含命令文本、备注、延时候选字段。
 * 新增时由外部生成 UUID（防止组件内多次渲染产生新 ID）。
 */
export default function CommandEditorModal({
  isOpen,
  editItem,
  defaultDelay,
  onSave,
  onClose,
}: CommandEditorModalProps) {
  const { t } = useTranslation();

  const [command, setCommand] = useState("");
  const [note, setNote] = useState("");
  const [delay, setDelay] = useState(defaultDelay);

  // 重置表单
  useEffect(() => {
    if (!isOpen) return;
    if (editItem) {
      setCommand(editItem.command);
      setNote(editItem.note);
      setDelay(editItem.delay);
    } else {
      setCommand("");
      setNote("");
      setDelay(defaultDelay);
    }
  }, [isOpen, editItem, defaultDelay]);

  const handleSave = useCallback(() => {
    if (!command.trim()) return;
    // 使用 crypto.randomUUID() 生成稳定 ID（现代浏览器 + Tauri 均支持）
    const id = editItem?.id ?? crypto.randomUUID();
    onSave({
      id,
      command: command.trim(),
      note: note.trim(),
      delay: Math.max(0, delay),
    });
    onClose();
  }, [command, note, delay, editItem, onSave, onClose]);

  const handleKeyDown = useCallback((e: React.KeyboardEvent) => {
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      handleSave();
    }
    if (e.key === "Escape") {
      onClose();
    }
  }, [handleSave, onClose]);

  const modalContent = (
    <AnimatePresence>
      {isOpen && (
        <motion.div
          className={`${styles.overlay} glass-overlay`}
          initial={{ opacity: 0 }}
          animate={{ opacity: 1 }}
          exit={{ opacity: 0 }}
          transition={{ duration: 0.15 }}
          onClick={(e) => { if (e.target === e.currentTarget) onClose(); }}
        >
          <motion.div
            className={`${styles.modal} liquid-glass`}
            initial={{ y: 10, scale: 0.96, opacity: 0 }}
            animate={{ y: 0, scale: 1, opacity: 1 }}
            exit={{ y: 10, scale: 0.96, opacity: 0 }}
            transition={{ duration: 0.12, ease: [0.4, 0, 0.2, 1] }}
            onClick={(e) => e.stopPropagation()}
          >
            <h3 className={styles.title}>
              {editItem
                ? (t("commandPanel.editCommandTitle"))
                : (t("commandPanel.addCommandTitle"))}
            </h3>

            <div className={styles.field}>
              <label className={styles.label}>
                {t("commandPanel.commandText")}
              </label>
              <textarea
                className={`${styles.textarea} liquid-glass-input`}
                value={command}
                onChange={(e) => setCommand(e.target.value)}
                onKeyDown={handleKeyDown}
                placeholder={t("commandPanel.commandPlaceholder")}
                rows={2}
                autoFocus
              />
            </div>

            <div className={styles.field}>
              <label className={styles.label}>
                {t("commandPanel.note")}
              </label>
              <input
                className={`${styles.input} liquid-glass-input`}
                type="text"
                value={note}
                onChange={(e) => setNote(e.target.value)}
                onKeyDown={handleKeyDown}
                placeholder={t("commandPanel.notePlaceholder")}
              />
            </div>

            <div className={styles.field}>
              <label className={styles.label}>
                {t("commandPanel.delay")}
              </label>
              <input
                className={`${styles.input} liquid-glass-input`}
                type="number"
                value={delay}
                onChange={(e) => setDelay(Math.max(0, Number(e.target.value)))}
                onKeyDown={handleKeyDown}
                min={0}
                max={60000}
                step={100}
              />
            </div>

            <div className={styles.actions}>
              <button
                className={`${styles.cancelBtn} liquid-glass-button`}
                onClick={onClose}
              >
                {t("common.cancel")}
              </button>
              <button
                className={`${styles.saveBtn} liquid-primary-button`}
                onClick={handleSave}
                disabled={!command.trim()}
              >
                {t("common.save")}
              </button>
            </div>
          </motion.div>
        </motion.div>
      )}
    </AnimatePresence>
  );

  return createPortal(modalContent, document.body);
}
