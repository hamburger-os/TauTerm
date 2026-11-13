/**
 * Which xterm renderer is actually drawing a terminal surface.
 *
 * `customGlyphs` is a WebGL-renderer feature: it is the only path that draws
 * block, quadrant and box-drawing characters as exact cell quads. The DOM
 * renderer paints those characters with whatever the font provides, so a silent
 * downgrade changes — and can break — pixel-art logos and TUI frames. The host
 * therefore records the outcome instead of discarding it: a downgraded surface
 * has to be explainable, not look like a terminal-emulation defect.
 *
 * Renderer *selection* stays in `TerminalSurface.tsx` (the single xterm owner);
 * this module owns only the vocabulary and the report helpers.
 */

export type TerminalRendererKind = "webgl" | "dom";

export const DOM_TERMINAL_RENDERER: TerminalRendererKind = "dom";

export function describeRendererFailure(error: unknown): string {
  if (error instanceof Error) return error.message;
  if (typeof error === "string") return error;
  return "unknown error";
}

/** Terminals that already reported a downgrade; keeps a retry loop quiet. */
const reported = new WeakSet<object>();

/**
 * Report a renderer downgrade once per terminal. A healthy WebGL surface stays
 * silent — this exists so an unavailable GPU path cannot fail invisibly again.
 */
export function warnRendererFallback(target: object, reason: string): void {
  if (reported.has(target)) return;
  reported.add(target);
  console.warn(
    `[tauterm] WebGL terminal renderer unavailable, falling back to the DOM renderer: ${reason}`,
  );
}
