export const SEND_BAR_MAX_TOTAL_RATIO = 0.8;

/**
 * Clamp the user-controlled SendBar body height in pixel space.
 *
 * The body minimum is the canonical CSS-derived geometry for the four vertical
 * mode buttons. TargetBar is a fixed additive row, so it is subtracted from the
 * total-height ceiling rather than folded into a percentage-based body size.
 */
export function clampSendBarBodyHeight(
  desiredBodyHeight: number,
  containerHeight: number,
  bodyMinHeight: number,
  targetBarHeight = 0,
): number {
  const minHeight = Number.isFinite(bodyMinHeight) && bodyMinHeight > 0
    ? bodyMinHeight
    : 0;
  const desiredHeight = Number.isFinite(desiredBodyHeight)
    ? desiredBodyHeight
    : minHeight;

  if (!Number.isFinite(containerHeight) || containerHeight <= 0) {
    return Math.max(minHeight, desiredHeight);
  }

  const fixedTargetHeight = Number.isFinite(targetBarHeight) && targetBarHeight > 0
    ? targetBarHeight
    : 0;
  const maxBodyHeight = Math.max(
    minHeight,
    containerHeight * SEND_BAR_MAX_TOTAL_RATIO - fixedTargetHeight,
  );

  return Math.min(maxBodyHeight, Math.max(minHeight, desiredHeight));
}

export function getSendBarHostHeightCss(
  bodyHeight: number | null,
  showTargetBar: boolean,
): string {
  const bodyHeightCss = bodyHeight === null
    ? "var(--sendbar-min-height)"
    : `${bodyHeight}px`;

  return showTargetBar
    ? `calc(${bodyHeightCss} + var(--sendbar-targetbar-height))`
    : bodyHeightCss;
}

export function getSendBarHostMinHeightCss(showTargetBar: boolean): string {
  return showTargetBar
    ? "calc(var(--sendbar-min-height) + var(--sendbar-targetbar-height))"
    : "var(--sendbar-min-height)";
}

export function getSendBarHostMaxHeightCss(): string {
  return `${SEND_BAR_MAX_TOTAL_RATIO * 100}%`;
}
