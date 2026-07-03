/**
 * 快捷键 Action ID 常量
 *
 * 集中定义所有快捷键动作标识符，避免魔术字符串分散在代码中。
 * 使用 `as const` 确保类型安全。
 */

export const ACTION_IDS = {
  // Session
  SESSION_NEW: "session.new",
  SESSION_CLOSE: "session.close",
  SESSION_NEXT: "session.next",
  SESSION_PREV: "session.prev",
  // Terminal (copy/paste handled by xterm.js natively, not via shortcut registry)
  TERMINAL_SEARCH: "terminal.search",

  // Application
  PALETTE_OPEN: "palette.open",
  SIDEBAR_TOGGLE: "sidebar.toggle",
  RIGHT_SIDEBAR_TOGGLE: "rightSidebar.toggle",
  SERIAL_REFRESH: "serial.refresh",
} as const;

export type ShortcutActionId = (typeof ACTION_IDS)[keyof typeof ACTION_IDS];
