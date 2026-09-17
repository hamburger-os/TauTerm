import { useCallback, useMemo, useRef } from "react";
import styles from "./TextEditor.module.css";

interface TextEditorProps {
  value: string;
  readOnly: boolean;
  autoFocus?: boolean;
  onChange: (value: string) => void;
  onSave: () => void;
  onCursorChange: (line: number, column: number) => void;
}

const MAX_GUTTER_LINES = 20_000;

export function countTextLines(text: string): number {
  let lines = 1;
  for (let index = 0; index < text.length; index += 1) {
    if (text.charCodeAt(index) === 10) lines += 1;
  }
  return lines;
}

function cursorPosition(text: string, offset: number): { line: number; column: number } {
  const safeOffset = Math.max(0, Math.min(offset, text.length));
  let line = 1;
  let lastLineBreak = -1;
  for (let index = 0; index < safeOffset; index += 1) {
    if (text.charCodeAt(index) === 10) {
      line += 1;
      lastLineBreak = index;
    }
  }
  return {
    line,
    column: safeOffset - lastLineBreak,
  };
}

export default function TextEditor({
  value,
  readOnly,
  autoFocus = false,
  onChange,
  onSave,
  onCursorChange,
}: TextEditorProps) {
  const textareaRef = useRef<HTMLTextAreaElement>(null);
  const gutterRef = useRef<HTMLPreElement>(null);
  const lineCount = useMemo(() => countTextLines(value), [value]);
  const showGutter = lineCount <= MAX_GUTTER_LINES;
  const gutter = useMemo(
    () => showGutter
      ? Array.from({ length: lineCount }, (_, index) => String(index + 1)).join("\n")
      : "",
    [lineCount, showGutter],
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
    // Plain Tab belongs to the editor as indentation. Shift+Tab is deliberately left to
    // normal focus navigation so keyboard users can move back to the document toolbar.
    if (
      readOnly
      || event.key !== "Tab"
      || event.shiftKey
      || event.ctrlKey
      || event.metaKey
      || event.altKey
    ) return;

    event.preventDefault();
    const textarea = event.currentTarget;
    const start = textarea.selectionStart;
    const end = textarea.selectionEnd;
    const next = `${value.slice(0, start)}\t${value.slice(end)}`;
    onChange(next);
    requestAnimationFrame(() => {
      textareaRef.current?.setSelectionRange(start + 1, start + 1);
      const cursor = cursorPosition(next, start + 1);
      onCursorChange(cursor.line, cursor.column);
    });
  }, [onChange, onCursorChange, onSave, readOnly, value]);

  return (
    <div className={`${styles.editor} ${showGutter ? "" : styles.editorWithoutGutter}`.trim()}>
      {showGutter && (
        <pre ref={gutterRef} className={styles.gutter} aria-hidden="true">{gutter}</pre>
      )}
      <textarea
        ref={textareaRef}
        className={styles.textarea}
        value={value}
        readOnly={readOnly}
        autoFocus={autoFocus}
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
