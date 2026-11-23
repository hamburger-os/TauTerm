/**
 * The single owner of "which terminal surface does a host-side terminal command
 * currently target".
 *
 * Terminal commands (copy / paste / search / select-all / clear / reset) are
 * registered once (see `useTerminalActions`) instead of per view: PTY terminals
 * live in the `TerminalView` pool while RTT renders the same shared surface from
 * its own plugin view, and a second dispatcher per view would mean duplicate
 * implementations and commands that silently no-op for one of them.
 *
 * Registration is activation-scoped, next to the focus owner in
 * `terminalFocus.ts`: only a surface that is active, visible and actually
 * measurable becomes the target, so a hidden Pane or background RTT channel can
 * never receive a clear/reset/copy.
 */

export interface TerminalCommandTarget {
  copySelection(): void;
  requestPaste(): void;
  selectAll(): void;
  toggleSearch(): void;
  /** Clear the screen and its scrollback (host-side, non-destructive to data sources). */
  clear(): void;
  /** Full terminal reset (RIS). */
  reset(): void;
}

let activeTarget: TerminalCommandTarget | null = null;

export function setActiveTerminalCommandTarget(target: TerminalCommandTarget): void {
  activeTarget = target;
}

/** Only the surface that is still the target may clear it (transitions may overlap). */
export function releaseActiveTerminalCommandTarget(target: TerminalCommandTarget): void {
  if (activeTarget === target) activeTarget = null;
}

export function getActiveTerminalCommandTarget(): TerminalCommandTarget | null {
  return activeTarget;
}
