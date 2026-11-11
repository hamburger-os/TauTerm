import type { IModes, Terminal } from "@xterm/xterm";

/**
 * Host-side view of the terminal's emulation state.
 *
 * xterm.js owns every VT mode bit; TauTerm only *reads* the sanctioned
 * `Terminal.modes` / active-buffer APIs so the Workspace can explain or recover
 * a full-screen application without parsing or rewriting PTY output.
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
