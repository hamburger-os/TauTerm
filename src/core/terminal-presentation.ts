import type { IModes, Terminal } from "@xterm/xterm";

/**
 * Host-side terminal commands + read-only view of the terminal's emulation state.
 *
 * xterm.js owns every VT mode bit and every byte-driven screen operation. The host
 * only *reads* the sanctioned `Terminal.modes` / active-buffer APIs and exposes the
 * two explicit local commands below; it never parses or rewrites PTY output.
 */
export interface TerminalPresentation {
  /** Active buffer is the alternate screen (`CSI ? 1049 h` and friends). */
  alternateScreen: boolean;
  /** Active mouse tracking protocol; `none` means the app did not capture the mouse. */
  mouseTrackingMode: IModes["mouseTrackingMode"];
}

export const IDLE_TERMINAL_PRESENTATION: TerminalPresentation = {
  alternateScreen: false,
  mouseTrackingMode: "none",
};

export function readTerminalPresentation(term: Terminal): TerminalPresentation {
  return {
    alternateScreen: term.buffer.active.type === "alternate",
    mouseTrackingMode: term.modes.mouseTrackingMode,
  };
}

export function isSameTerminalPresentation(
  left: TerminalPresentation,
  right: TerminalPresentation,
): boolean {
  return left.alternateScreen === right.alternateScreen
    && left.mouseTrackingMode === right.mouseTrackingMode;
}

/**
 * Full terminal reset (RIS). xterm owns the sequence; TauTerm never writes
 * reset bytes into the PTY itself. Used only by the explicit "Reset Terminal"
 * command after a full-screen application left the surface in a stuck state.
 */
export function resetTerminalSurface(term: Terminal): void {
  term.reset();
}

/**
 * Clear the screen *and* its scrollback, keeping the cursor line as the new first
 * line (xterm's "clear the entire buffer"). This is the host-side command, not a
 * byte-stream clear: a remote `clear` follows VT semantics (ED2 erases only the
 * viewport, ED3 erases saved lines), so the two must never be conflated. Data
 * sources that can restore their own history on remount (RTT) are told to drop
 * their replay window separately via `TerminalSurfaceSource.clearReplayHistory`.
 */
export function clearTerminalSurface(term: Terminal): void {
  term.clear();
}
