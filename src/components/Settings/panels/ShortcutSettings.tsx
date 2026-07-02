import { useState, useEffect, useCallback, useRef, useMemo } from "react";
import { useTranslation } from "react-i18next";
import { shortcutRegistry, buildKeyString } from "../../../shortcuts/registry";
import type { ShortcutActionId } from "../../../shortcuts/actionIds";
import { isInputFocused } from "../../../utils/dom";
import styles from "./ShortcutSettings.module.css";
import settingsStyles from "../SettingsPage.module.css";
import Icon from "../../common/Icon";
import GlassButton from "../../common/GlassButton";

/** 用 "+" 分隔的按键串渲染为视觉标签，如 "Ctrl + Shift + N" */
function formatKeys(keys: string): string {
  return keys.split("+").join(" + ");
}

export default function ShortcutSettings() {
  const { t } = useTranslation();
  // 版本计数器：更新/重置后 +1 以触发重新渲染（getByCategory 从 registry 实时读取）
  const [version, setVersion] = useState(0);
  const [recordingId, setRecordingId] = useState<ShortcutActionId | null>(null);
  const [conflictId, setConflictId] = useState<ShortcutActionId | null>(null);
  const [conflictMsg, setConflictMsg] = useState("");
  const conflictTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  // 标记版本已刷新（用于触发 useMemo 重新计算 grouped）
  const refreshShortcuts = useCallback(() => {
    setVersion(v => v + 1);
  }, []);

  // 冲突提示定时器清理
  useEffect(() => {
    return () => {
      if (conflictTimerRef.current) clearTimeout(conflictTimerRef.current);
    };
  }, []);

  const handleRowClick = useCallback((id: ShortcutActionId) => {
    if (recordingId !== null || conflictId !== null) return;
    if (isInputFocused()) return;
    // 清除残留的冲突定时器，防止旧 timer 在新的录制中触发导致状态异常
    if (conflictTimerRef.current) {
      clearTimeout(conflictTimerRef.current);
      conflictTimerRef.current = null;
    }
    setRecordingId(id);
  }, [recordingId, conflictId]);

  // 录制：监听 keydown
  useEffect(() => {
    if (recordingId === null) return;

    const handleKeyDown = (e: KeyboardEvent) => {
      // Esc 取消录制（不关闭设置页面）
      if (e.key === "Escape") {
        e.preventDefault();
        e.stopPropagation();
        setRecordingId(null);
        return;
      }

      // 忽略输入框内的按键
      if (isInputFocused()) return;

      // 构建按键字符串
      const newKeys = buildKeyString(e);
      if (!newKeys) return; // 仅修饰键，忽略

      e.preventDefault();
      e.stopPropagation();

      // 执行更新（含冲突检测）
      const conflict = shortcutRegistry.update(recordingId, newKeys);
      if (conflict) {
        // 冲突：显示提示
        // 先清除旧的冲突定时器，防止多个冲突提示相互覆盖
        if (conflictTimerRef.current) clearTimeout(conflictTimerRef.current);
        setConflictId(recordingId);
        setConflictMsg(t("settings.shortcutsConflict", { name: conflict }));
        setRecordingId(null);

        conflictTimerRef.current = setTimeout(() => {
          setConflictId(null);
          setConflictMsg("");
        }, 1500);
      } else {
        // 成功
        setRecordingId(null);
        refreshShortcuts();
      }
    };

    document.addEventListener("keydown", handleKeyDown, true);
    return () => document.removeEventListener("keydown", handleKeyDown, true);
  }, [recordingId, refreshShortcuts, t]);

  const handleResetAll = useCallback(() => {
    shortcutRegistry.resetAll();
    refreshShortcuts();
  }, [refreshShortcuts]);

  // 按分类分组（使用 registry 方法，version 变化时重新计算）
  // eslint-disable-next-line react-hooks/exhaustive-deps
  const grouped = useMemo(() => shortcutRegistry.getByCategory(), [version]);

  const categoryOrder = ["Session", "Terminal", "Application"];
  const categoryLabelMap: Record<string, string> = {
    Session: t("settings.shortcutsCategory_session"),
    Terminal: t("settings.shortcutsCategory_terminal"),
    Application: t("settings.shortcutsCategory_application"),
  };

  return (
    <div className={recordingId !== null ? styles.recordingMode : undefined}>
      <h3 className={settingsStyles.panelTitle}>{t("settings.shortcuts")}</h3>
      <p className={styles.panelDesc}>{t("settings.shortcutsDesc")}</p>

      {categoryOrder.map(cat => {
        const items = grouped.get(cat);
        if (!items || items.length === 0) return null;
        return (
          <div key={cat}>
            <h4 className={styles.categoryTitle}>{categoryLabelMap[cat] || cat}</h4>
            {items.map(s => {
              const isRecording = recordingId === s.id;
              const isConflict = conflictId === s.id;
              return (
                <div
                  key={s.id}
                  className={`${styles.shortcutRow} ${
                    isRecording ? styles.shortcutRowRecording : ""
                  } ${isConflict ? styles.shortcutRowConflict : ""}`}
                  onClick={() => handleRowClick(s.id as ShortcutActionId)}
                >
                  <span className={styles.shortcutLabel}>{s.descriptionKey ? t(s.descriptionKey) : s.description}</span>
                  <span
                    className={`${styles.shortcutKeys} ${
                      isRecording ? styles.shortcutKeysRecording : ""
                    } ${isConflict ? styles.shortcutKeysConflict : ""}`}
                  >
                    {isRecording
                      ? t("settings.shortcutsRecording")
                      : isConflict
                        ? conflictMsg
                        : formatKeys(s.keys)}
                  </span>
                </div>
              );
            })}
          </div>
        );
      })}

      {/* 重置按钮 */}
      <div className={styles.resetArea}>
        <GlassButton variant="secondary" size="sm" onClick={handleResetAll}>
          <Icon name="refresh" size="sm" />
          {t("settings.shortcutsReset")}
        </GlassButton>
      </div>
    </div>
  );
}
