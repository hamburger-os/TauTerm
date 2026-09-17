import { useCallback, useMemo, useRef } from "react";
import styles from "./TextEditor.module.css";

interface TextEditorProps {
  value: string;
  readOnly: boolean;
  onChange: (value: string) => void;
  onSave: () => void;
  onCursorChange: (line: number, column: number) => void;
}

function cursorPosition(text: string, offset: number): { line: number; column: number } {
  const safeOffset = Math.max(0, Math.min(offset, text.length));
  const before = text.slice(0, safeOffset);
  const lines = before.split("\n");
  return {
    line: lines.length,
    column: (lines[lines.length - 1]?.length ?? 0) + 1,
  };
}

export default function TextEditor({
  value,
  readOnly,
  onChange,
  onSave,
  onCursorChange,
}: TextEditorProps) {
  const textareaRef = useRef<HTMLTextAreaElement>(null);
  const gutterRef = useRef<HTMLPreElement>(null);
  const lineCount = useMemo(() => Math.max(1, value.split("\n").length), [value]);
  const gutter = useMemo(
    () => Array.from({ length: lineCount }, (_, index) => String(index + 1)).join("\n"),
    [lineCount],
  );

  const reportCursor = useCallback(() => {
    const textarea = textareaRef.current;
    if (!textarea) return;
    const cursor = cursorPosition(value, textarea.selectionStart);
    onCursorChange(cursor.line, cursor.column);
  }, [onCursorChange, value]);

  const handleKeyDown = useCallback((event: React.KeyboardEvent<HTMLTextAreaElement>) => {
    if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === "s") {
      event.preventDefault();
      onSave();
      return;
    }
    if (readOnly || event.key !== "Tab" || event.ctrlKey || event.metaKey || event.altKey) return;

    event.preventDefault();
    const textarea = event.currentTarget;
    const start = textarea.selectionStart;
    const end = textarea.selectionEnd;
    const next = `${value.slice(0, start)}\t${value.slice(end)}`;
    onChange(next);
    requestAnimationFrame(() => {
      textareaRef.current?.setSelectionRange(start + 1, start + 1);
      onCursorChange(
        cursorPosition(next, start + 1).line,
        cursorPosition(next, start + 1).column,
      );
    });
  }, [onChange, onCursorChange, onSave, readOnly, value]);

  return (
    <div className={styles.editor}>
      <pre ref={gutterRef} className={styles.gutter} aria-hidden="true">{gutter}</pre>
      <textarea
        ref={textareaRef}
        className={styles.textarea}
        value={value}
        readOnly={readOnly}
        wrap="off"
        spellCheck={false}
        autoCapitalize="off"
        autoCorrect="off"
        onChange={(event) => onChange(event.target.value)}
        onKeyDown={handleKeyDown}
        onClick={reportCursor}
        onKeyUp={reportCursor}
        onSelect={reportCursor}
        onScroll={(event) => {
          if (gutterRef.current) gutterRef.current.scrollTop = event.currentTarget.scrollTop;
        }}
        aria-label="Remote document text editor"
      />
    </div>
  );
}
