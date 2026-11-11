/**
 * Device-pixel alignment for xterm's DOM renderer.
 *
 * xterm sizes each DOM row from an integer *device-pixel* cell height and then
 * divides it by the device pixel ratio to get a CSS row height. At fractional
 * display scales an odd device cell becomes a fractional CSS row height (200%
 * scale with a 33px device cell => 16.5px rows), so every row sits on a half
 * pixel phase, per-row clipping splits vertical glyph columns, and TUI borders
 * or character scrollbars render as dashes.
 *
 * `planIntegerCellHeight` chooses the smallest `lineHeight` whose device cell
 * height is a whole DPR quantum, which makes the CSS row height an integer
 * without changing font size or letter spacing. It returns null when the current
 * configuration is already aligned or cannot be aligned safely.
 */

/** Smallest integer `q` for which `devicePixelRatio * q` is integral. */
export function devicePixelQuantum(devicePixelRatio: number): number | null {
  if (!Number.isFinite(devicePixelRatio) || devicePixelRatio <= 0) return null;
  for (let quantum = 1; quantum <= 8; quantum += 1) {
    if (Math.abs(devicePixelRatio * quantum - Math.round(devicePixelRatio * quantum)) < 1e-6) {
      return quantum;
    }
  }
  return null;
}

export interface CellHeightInput {
  /** Currently applied xterm `lineHeight` option. */
  lineHeight: number;
  /** Font box height (ascent + descent) in CSS pixels, as xterm measures it. */
  fontBoxHeight: number;
  devicePixelRatio: number;
}

export interface CellHeightPlan {
  /** New `lineHeight` option value. */
  lineHeight: number;
  /** Resulting CSS row height in whole pixels. */
  cssCellHeight: number;
  /** Resulting device-pixel row height. */
  deviceCellHeight: number;
}

const MIN_LINE_HEIGHT = 0.9;
const MAX_LINE_HEIGHT = 1.35;

export function planIntegerCellHeight(input: CellHeightInput): CellHeightPlan | null {
  const { lineHeight, fontBoxHeight, devicePixelRatio: dpr } = input;
  if (!(fontBoxHeight > 0) || !(dpr > 0)) return null;
  const quantum = devicePixelQuantum(dpr);
  if (quantum === null) return null;

  const deviceCharHeight = Math.ceil(fontBoxHeight * dpr);
  if (deviceCharHeight <= 0) return null;

  const appliedLineHeight = lineHeight > 0 ? lineHeight : 1;
  const currentDeviceCell = Math.floor(deviceCharHeight * appliedLineHeight);
  if (Math.abs(currentDeviceCell / dpr - Math.round(currentDeviceCell / dpr)) < 1e-6) {
    // Already on the device-pixel grid (common at 100% scale).
    return null;
  }

  const cssCellHeight = Math.ceil(deviceCharHeight / dpr / quantum) * quantum;
  const deviceCellHeight = cssCellHeight * dpr;
  const exactLineHeight = (deviceCellHeight + 0.5) / deviceCharHeight;
  if (!Number.isFinite(exactLineHeight)) return null;
  if (exactLineHeight < MIN_LINE_HEIGHT || exactLineHeight > MAX_LINE_HEIGHT) return null;

  const roundedLineHeight = Number(exactLineHeight.toFixed(4));
  // xterm computes `floor(deviceCharHeight * lineHeight)`; the +0.5 margin above
  // keeps that floor stable across the 4-decimal rounding.
  if (Math.floor(deviceCharHeight * roundedLineHeight) !== deviceCellHeight) return null;
  if (Math.abs(roundedLineHeight - appliedLineHeight) < 1e-4) return null;

  return { lineHeight: roundedLineHeight, cssCellHeight, deviceCellHeight };
}
