export interface TerminalPasteAnalysis {
  requiresConfirmation: boolean;
  contentLineCount: number;
  characterCount: number;
}

/** Normalize clipboard newlines without changing any other pasted content. */
export function normalizeTerminalPasteText(text: string): string {
  return text.replace(/\r\n?/g, "\n");
}

/**
 * Multi-line paste safety rule.
 *
 * A confirmation is required only when the clipboard contains at least two
 * non-empty logical lines. A normal single line, including one trailing
 * newline, stays frictionless.
 */
export function analyzeTerminalPaste(text: string): TerminalPasteAnalysis {
  const normalized = normalizeTerminalPasteText(text);
  const contentLineCount = normalized
    .split("\n")
    .filter(line => line.trim().length > 0)
    .length;

  return {
    requiresConfirmation: contentLineCount >= 2,
    contentLineCount,
    characterCount: text.length,
  };
}

export function buildTerminalPastePreview(
  text: string,
  maxLines = 8,
  maxCharacters = 1200,
): { preview: string; truncated: boolean } {
  const normalized = normalizeTerminalPasteText(text);
  const lines = normalized.split("\n");
  const selected = lines.slice(0, Math.max(1, maxLines)).join("\n");
  const byLinesTruncated = lines.length > maxLines;
  const byCharactersTruncated = selected.length > maxCharacters;
  const preview = selected.slice(0, maxCharacters);

  return {
    preview,
    truncated: byLinesTruncated || byCharactersTruncated,
  };
}
