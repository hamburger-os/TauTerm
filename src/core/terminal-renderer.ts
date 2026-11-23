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
 * Renderer *mechanics* (addon construction, dispose, context-loss handling) stay in
 * `TerminalSurface.tsx`, the single xterm owner. This module owns the vocabulary:
 * which renderer is actually drawing, the user-visible policy (`auto` | `cpu`), and
 * the predicate that decides whether a policy enables WebGL for a displayed surface.
 */

export type TerminalRendererKind = "webgl" | "dom";

export const DOM_TERMINAL_RENDERER: TerminalRendererKind = "dom";

/**
 * 用户可见的渲染器策略。
 *
 * 只有两个值，因为它们才有不同的行为：`auto` 优先 GPU 并在 WebGL 不可用时
 * 回退 CPU；`cpu` 完全关闭 WebGL。不存在第三个“强制 GPU”——那只是 `auto`
 * 已经在做的事，多一个同义选项只会让设置页说谎。
 */
export type TerminalRendererPolicy = "auto" | "cpu";

export const DEFAULT_TERMINAL_RENDERER_POLICY: TerminalRendererPolicy = "auto";

export function isTerminalRendererPolicy(value: unknown): value is TerminalRendererPolicy {
  return value === "auto" || value === "cpu";
}

/**
 * 该策略下是否应该启用 GPU 渲染器。
 *
 * `displayed` 由 surface 提供：只有真正显示出来的终端才值得占用 GPU 上下文，
 * 池中隐藏的终端继续用 DOM 渲染器，把 WebGL 上下文数量限制在可见 Pane 数内。
 */
export function shouldUseWebglRenderer(
  policy: TerminalRendererPolicy,
  displayed: boolean,
): boolean {
  return policy === "auto" && displayed;
}

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
