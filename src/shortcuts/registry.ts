export interface ShortcutAction {
  id: string;
  keys: string; // e.g., "Ctrl+Shift+N"
  description: string;
  category: string;
}

/**
 * 快捷键注册表
 *
 * 集中管理所有快捷键绑定。添加冲突检测。
 */
class ShortcutRegistry {
  private shortcuts: Map<string, ShortcutAction> = new Map();

  register(shortcut: ShortcutAction): void {
    // 冲突检测
    for (const [existingId, existing] of this.shortcuts) {
      if (existing.keys === shortcut.keys && existingId !== shortcut.id) {
        console.warn(
          `[ShortcutRegistry] Shortcut conflict: "${shortcut.keys}" is already used by "${existingId}", overwriting with "${shortcut.id}"`
        );
      }
    }
    this.shortcuts.set(shortcut.id, shortcut);
  }

  /**
   * 匹配键盘事件到注册的快捷键
   */
  match(event: KeyboardEvent): ShortcutAction | null {
    const pressed = this.buildKeyString(event);
    if (!pressed) return null;

    for (const shortcut of this.shortcuts.values()) {
      if (this.keysMatch(pressed, shortcut.keys)) {
        return shortcut;
      }
    }
    return null;
  }

  private buildKeyString(event: KeyboardEvent): string {
    const parts: string[] = [];
    if (event.ctrlKey || event.metaKey) parts.push("Ctrl");
    if (event.shiftKey) parts.push("Shift");
    if (event.altKey) parts.push("Alt");

    const key = event.key;
    // 跳过仅修饰键的按键
    if (key === "Control" || key === "Shift" || key === "Alt" || key === "Meta") {
      return "";
    }

    // 规范化键名
    const keyMap: Record<string, string> = {
      ArrowUp: "Up", ArrowDown: "Down", ArrowLeft: "Left", ArrowRight: "Right",
      " ": "Space", Escape: "Esc",
    };
    parts.push(keyMap[key] || (key.length === 1 ? key.toUpperCase() : key));

    return parts.join("+");
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

  getByCategory(): Map<string, ShortcutAction[]> {
    const grouped = new Map<string, ShortcutAction[]>();
    for (const s of this.shortcuts.values()) {
      const list = grouped.get(s.category) || [];
      list.push(s);
      grouped.set(s.category, list);
    }
    return grouped;
  }
}

export const shortcutRegistry = new ShortcutRegistry();

// ── 注册默认快捷键 ───────────────────────────────────

shortcutRegistry.register({ id: "session.new", keys: "Ctrl+Shift+N", description: "新建会话", category: "Session" });
shortcutRegistry.register({ id: "session.close", keys: "Ctrl+Shift+W", description: "关闭当前会话", category: "Session" });
shortcutRegistry.register({ id: "session.next", keys: "Ctrl+Tab", description: "下一个标签页", category: "Session" });
shortcutRegistry.register({ id: "session.prev", keys: "Ctrl+Shift+Tab", description: "上一个标签页", category: "Session" });
shortcutRegistry.register({ id: "session.tab1", keys: "Alt+1", description: "切换到标签页 1", category: "Session" });
shortcutRegistry.register({ id: "session.tab2", keys: "Alt+2", description: "切换到标签页 2", category: "Session" });
shortcutRegistry.register({ id: "session.tab3", keys: "Alt+3", description: "切换到标签页 3", category: "Session" });

shortcutRegistry.register({ id: "terminal.search", keys: "Ctrl+F", description: "终端搜索", category: "Terminal" });
shortcutRegistry.register({ id: "terminal.copy", keys: "Ctrl+Shift+C", description: "复制", category: "Terminal" });
shortcutRegistry.register({ id: "terminal.paste", keys: "Ctrl+Shift+V", description: "粘贴", category: "Terminal" });

shortcutRegistry.register({ id: "transfer.toggle", keys: "Ctrl+Shift+F", description: "切换文件传输面板", category: "Transfer" });

shortcutRegistry.register({ id: "palette.open", keys: "Ctrl+Shift+P", description: "打开命令面板", category: "Application" });
shortcutRegistry.register({ id: "sidebar.toggle", keys: "Ctrl+Shift+B", description: "切换侧边栏", category: "Application" });
shortcutRegistry.register({ id: "serial.refresh", keys: "Ctrl+Shift+R", description: "刷新端口列表", category: "Application" });
