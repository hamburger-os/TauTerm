import { useEffect } from "react";
import { useKeyboard } from "./useKeyboard";
import { ACTION_IDS } from "../shortcuts/actionIds";
import { getActiveTerminalCommandTarget } from "../components/Terminal/terminalCommandTarget";

/**
 * Registers the host-side terminal commands exactly once for the whole app.
 *
 * The target is resolved through `terminalCommandTarget` rather than through a
 * view's private terminal pool: the shared surface serves PTY sessions, RTT
 * channels and protocol views alike, and a command must follow the terminal the
 * user is actually looking at. No active target simply means a no-op.
 *
 * Keyboard, command palette and context menus all end up here (or in the shared
 * surface itself), so there is one implementation per command and one owner of
 * the action id.
 */
export function useTerminalActions(): void {
  const { registerAction } = useKeyboard();

  useEffect(() => {
    const target = () => getActiveTerminalCommandTarget();
    registerAction(ACTION_IDS.TERMINAL_COPY, () => target()?.copySelection());
    registerAction(ACTION_IDS.TERMINAL_PASTE, () => target()?.requestPaste());
    registerAction(ACTION_IDS.TERMINAL_SEARCH, () => target()?.toggleSearch());
    registerAction(ACTION_IDS.TERMINAL_SELECT_ALL, () => target()?.selectAll());
    registerAction(ACTION_IDS.TERMINAL_CLEAR, () => target()?.clear());
    registerAction(ACTION_IDS.TERMINAL_RESET, () => target()?.reset());
  }, [registerAction]);
}
