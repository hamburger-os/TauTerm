import { ACTION_IDS, type ShortcutActionId } from "./actionIds";

/** 从 KeyboardEvent 构建按键字符串（如 "Ctrl+Shift+N"），供 ShortcutRegistry 和 ShortcutSettings 共用 */
export function buildKeyString(event: KeyboardEvent): string {
  const parts: string[] = [];
  if (event.ctrlKey || event.metaKey) parts.push("Ctrl");
  if (event.shiftKey) parts.push("Shift");
  if (event.altKey) parts.push("Alt");

  const key = event.key;
  if (key === "Control" || key === "Shift" || key === "Alt" || key === "Meta") {
    return "";
  }

  const keyMap: Record<string, string> = {
    ArrowUp: "Up", ArrowDown: "Down", ArrowLeft: "Left", ArrowRight: "Right",
    " ": "Space", Escape: "Esc",
  };
  parts.push(keyMap[key] || (key.length === 1 ? key.toUpperCase() : key));

  return parts.join("+");
}

export interface ShortcutAction {
  id: ShortcutActionId;
  keys: string; // e.g., "Ctrl+Shift+N"
  descriptionKey?: string; // i18n key for the description (preferred for display)
  description: string;     // fallback display text
  category: string;
}

/**
 * 默认快捷键配置（不可变，用于"重置为默认值"）。
 *
 * 注意：Ctrl+Shift+C（复制）和 Ctrl+Shift+V（粘贴）由 xterm.js 原生处理，
 * 不经过 shortcut registry 匹配，因此不出现在此列表中。
 */
export const DEFAULT_SHORTCUTS: ShortcutAction[] = [
  // Session
  { id: ACTION_IDS.SESSION_NEW, keys: "Ctrl+Shift+N", descriptionKey: "settings.shortcutsAction_newSession", description: "新建会话", category: "Session" },
  { id: ACTION_IDS.SESSION_CLOSE, keys: "Ctrl+Shift+W", descriptionKey: "settings.shortcutsAction_closeSession", description: "关闭当前会话", category: "Session" },
  { id: ACTION_IDS.SESSION_NEXT, keys: "Ctrl+Tab", descriptionKey: "settings.shortcutsAction_nextTab", description: "下一个标签页", category: "Session" },
  { id: ACTION_IDS.SESSION_PREV, keys: "Ctrl+Shift+Tab", descriptionKey: "settings.shortcutsAction_prevTab", description: "上一个标签页", category: "Session" },
  // Terminal
  { id: ACTION_IDS.TERMINAL_SEARCH, keys: "Ctrl+F", descriptionKey: "settings.shortcutsAction_terminalSearch", description: "终端搜索", category: "Terminal" },
  // Application
  { id: ACTION_IDS.PALETTE_OPEN, keys: "Ctrl+Shift+P", descriptionKey: "settings.shortcutsAction_openPalette", description: "打开命令面板", category: "Application" },
  { id: ACTION_IDS.SIDEBAR_TOGGLE, keys: "Ctrl+Shift+B", descriptionKey: "settings.shortcutsAction_toggleSidebar", description: "切换侧边栏", category: "Application" },
  { id: ACTION_IDS.SERIAL_REFRESH, keys: "Ctrl+Shift+R", descriptionKey: "settings.shortcutsAction_refreshPorts", description: "刷新端口列表", category: "Application" },
];

const STORAGE_KEY = "tauterm-shortcuts";

/**
 * 快捷键注册表
 *
 * 集中管理所有快捷键绑定。支持自定义绑定、冲突检测、localStorage 持久化。
 */
class ShortcutRegistry {
  private shortcuts: Map<string, ShortcutAction> = new Map();

  constructor() {
    this.loadFromStorage();
  }

  /**
   * 更新已有快捷键的按键绑定。
   * 返回冲突的动作描述，若无冲突返回 null。
   */
  update(id: ShortcutActionId, newKeys: string): string | null {
    const target = this.shortcuts.get(id);
    if (!target) return null;

    // 冲突检测：检查 newKeys 是否已被其他动作占用
    for (const [existingId, existing] of this.shortcuts) {
      if (existingId !== id && existing.keys === newKeys) {
        return existing.description;
      }
    }

    // 无冲突，更新
    target.keys = newKeys;
    this.saveToStorage();
    return null;
  }

  /**
   * 匹配键盘事件到注册的快捷键
   */
  match(event: KeyboardEvent): ShortcutAction | null {
    const pressed = buildKeyString(event);
    if (!pressed) return null;

    for (const shortcut of this.shortcuts.values()) {
      if (this.keysMatch(pressed, shortcut.keys)) {
        return shortcut;
      }
    }
    return null;
  }

  private keysMatch(pressed: string, registered: string): boolean {
    const pressedParts = pressed.split("+").sort();
    const registeredParts = registered.split("+").sort();
    if (pressedParts.length !== registeredParts.length) return false;
    return pressedParts.every((p, i) => p === registeredParts[i]);
  }

  getAll(): ShortcutAction[] {
    return Array.from(this.shortcuts.values());
  }

  /** 按 category 分组返回快捷键，用于设置面板展示 */
  getByCategory(): Map<string, ShortcutAction[]> {
    const grouped = new Map<string, ShortcutAction[]>();
    for (const s of this.shortcuts.values()) {
      const list = grouped.get(s.category) || [];
      list.push(s);
      grouped.set(s.category, list);
    }
    return grouped;
  }

  /** 重置所有快捷键为默认值 */
  resetAll(): void {
    this.shortcuts.clear();
    for (const s of DEFAULT_SHORTCUTS) {
      this.shortcuts.set(s.id, { ...s });
    }
    this.saveToStorage();
  }

  // ── 持久化 ───────────────────────────────────

  private loadFromStorage(): void {
    // 合法的 action ID 集合（用于过滤已删除的旧快捷键）
    const validIds = new Set(DEFAULT_SHORTCUTS.map(s => s.id));

    try {
      const raw = localStorage.getItem(STORAGE_KEY);
      if (raw) {
        const data: ShortcutAction[] = JSON.parse(raw);
        let hasInvalid = false;
        for (const s of data) {
          if (validIds.has(s.id)) {
            this.shortcuts.set(s.id, s);
          } else {
            hasInvalid = true;
          }
        }
        if (hasInvalid) {
          // 清除包含已删除 action 的旧缓存，重新保存
          this.saveToStorage();
        }
        return;
      }
    } catch (e) {
      console.warn("[ShortcutRegistry] Failed to load from localStorage, using defaults", e);
    }
    // 无存储数据或解析失败 → 使用默认值
    this.loadDefaults();
  }

  private loadDefaults(): void {
    for (const s of DEFAULT_SHORTCUTS) {
      this.shortcuts.set(s.id, { ...s });
    }
  }

  private saveToStorage(): void {
    try {
      const data = Array.from(this.shortcuts.values());
      localStorage.setItem(STORAGE_KEY, JSON.stringify(data));
    } catch (e) {
      console.warn("[ShortcutRegistry] Failed to save to localStorage", e);
    }
  }
}

export const shortcutRegistry = new ShortcutRegistry();
