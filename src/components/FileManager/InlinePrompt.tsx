/**
 * 内联输入提示组件
 *
 * 用于新建文件/文件夹、重命名等场景。
 * 显示一个带确认/取消按钮的输入框。
 */
import { useState, useRef, useEffect, useCallback } from "react";
import { useTranslation } from "react-i18next";
import styles from "./InlinePrompt.module.css";

interface InlinePromptProps {
  visible: boolean;
  defaultValue?: string;
  placeholder?: string;
  onConfirm: (value: string) => void;
  onCancel: () => void;
}

export default function InlinePrompt({
  visible,
  defaultValue = "",
  placeholder,
  onConfirm,
  onCancel,
}: InlinePromptProps) {
  const { t } = useTranslation();
  const [value, setValue] = useState(defaultValue);
  const inputRef = useRef<HTMLInputElement>(null);

  // 重置输入值并在可见时自动聚焦
  useEffect(() => {
    if (visible) {
      setValue(defaultValue);
      // 延迟聚焦以确保 DOM 已渲染
      const timer = setTimeout(() => {
        inputRef.current?.focus();
        inputRef.current?.select();
      }, 10);
      return () => clearTimeout(timer);
    }
  }, [visible, defaultValue]);

  const handleConfirm = useCallback(() => {
    const trimmed = value.trim();
    if (trimmed) {
      onConfirm(trimmed);
    }
  }, [value, onConfirm]);

  const handleKeyDown = useCallback(
    (e: React.KeyboardEvent) => {
      if (e.key === "Enter") {
        handleConfirm();
      } else if (e.key === "Escape") {
        onCancel();
      }
    },
    [handleConfirm, onCancel]
  );

  if (!visible) return null;

  return (
    <div className={styles.container}>
      <input
        ref={inputRef}
        className={styles.input}
        type="text"
        value={value}
        placeholder={placeholder}
        onChange={(e) => setValue(e.target.value)}
        onKeyDown={handleKeyDown}
      />
      <button className={styles.btn} onClick={handleConfirm}>
        {t("common.ok")}
      </button>
      <button className={styles.btn} onClick={onCancel}>
        {t("common.cancel")}
      </button>
    </div>
  );
}
