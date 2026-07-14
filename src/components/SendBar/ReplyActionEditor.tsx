import { Fragment, useRef } from "react";
import { useTranslation } from "react-i18next";
import type { ReplyAction } from "./types";
import { clampNumber } from "./numInput";
import { usePointerDragReorder } from "../../hooks/usePointerDragReorder";
import MacroPicker from "./MacroPicker";
import Icon from "../common/Icon";
import styles from "./AutoReplyRuleEditor.module.css";

interface ReplyActionEditorProps {
  actions: ReplyAction[];
  onChange: (actions: ReplyAction[]) => void;
}

export default function ReplyActionEditor({ actions, onChange }: ReplyActionEditorProps) {
  const { t } = useTranslation();

  // ── 拖拽排序（使用通用 hook）──
  const listRef = useRef<HTMLDivElement>(null);
  const {
    isDragging,
    dropIndex,
    handlePointerDown,
    handlePointerMove,
    handlePointerUp,
    handlePointerCancel,
  } = usePointerDragReorder(actions, onChange, {
    itemSelector: `.${styles.actionRow}`,
    draggingClass: styles.actionRowDragging,
    listRef,
  });

  const handleAdd = () => {
    onChange([...actions, { delayMs: 0, data: "", format: "text" }]);
  };

  const handleRemove = (idx: number) => {
    onChange(actions.filter((_, i) => i !== idx));
  };

  const handleChange = (idx: number, field: keyof ReplyAction, value: unknown) => {
    const next = actions.map((a, i) =>
      i === idx ? { ...a, [field]: value } : a
    );
    onChange(next);
  };

  return (
    <div
      ref={listRef}
      className={`${styles.actionEditor} ${isDragging ? styles.actionListDragging : ""}`}
    >
      {actions.length === 0 && (
        <div className={styles.actionEmpty}>
          {t("sendBar.noActions")}
        </div>
      )}
      {actions.map((action, idx) => (
        <Fragment key={idx}>
          {isDragging && dropIndex === idx && (
            <div className={styles.actionDropIndicator} />
          )}
          <div className={styles.actionRow}>
            <span
              className={styles.actionDragHandle}
              title={t("commandPanel.dragToReorder")}
              onPointerDown={(e) => handlePointerDown(e, idx)}
              onPointerMove={handlePointerMove}
              onPointerUp={handlePointerUp}
              onPointerCancel={handlePointerCancel}
              style={{ touchAction: "none" }}
            >
              <Icon name="drag-handle" size={14} color="currentColor" />
            </span>
            <button
              type="button"
              className={`${styles.actionRemove} liquid-glass-button`}
              onClick={() => handleRemove(idx)}
              title={t("sendBar.removeAction")}
            >
              <Icon name="close" size="sm" />
            </button>
            <div className={styles.field}>
              <label>{t("sendBar.replyData")}</label>
              <textarea
                className={`${styles.replyTextarea} liquid-glass-input`}
                value={action.data}
                onChange={e => handleChange(idx, "data", e.target.value)}
                placeholder="e.g. OK\r\n  ({{MACRO}} supported)"
                rows={4}
              />
              <MacroPicker
                value={action.data}
                onChange={(val) => handleChange(idx, "data", val)}
              />
            </div>
            <div className={styles.fieldRow}>
              <div className={styles.field}>
                <label>{t("sendBar.delayMs")}</label>
                <input
                  className="liquid-glass-input"
                  type="number"
                  value={action.delayMs}
                  onChange={e => handleChange(idx, "delayMs", clampNumber(e.target.value, 0, 60000))}
                  min={0}
                  max={60000}
                  step={100}
                />
              </div>
              <div className={styles.field}>
                <label>{t("sendBar.replyFormat")}</label>
                <select
                  className="liquid-glass-input liquid-glass-select"
                  value={action.format}
                  onChange={e => handleChange(idx, "format", e.target.value)}
                >
                  <option value="text">Text</option>
                  <option value="hex">HEX</option>
                </select>
              </div>
            </div>
          </div>
        </Fragment>
      ))}
      {isDragging && dropIndex === actions.length && actions.length > 0 && (
        <div className={styles.actionDropIndicator} />
      )}
      <button
        type="button"
        className={`${styles.actionAdd} liquid-glass-button`}
        onClick={handleAdd}
      >
        <Icon name="plus" size="sm" />
        {t("sendBar.addAction")}
      </button>
    </div>
  );
}
