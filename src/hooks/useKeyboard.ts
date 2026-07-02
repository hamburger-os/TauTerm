import { useEffect, useCallback, useRef } from "react";
import { shortcutRegistry } from "../shortcuts/registry";
import type { ShortcutActionId } from "../shortcuts/actionIds";
import { isInputFocused } from "../utils/dom";

/**
 * 全局快捷键监听 hook
 *
 * 在 document 级别监听键盘事件，匹配注册的快捷键并执行对应 action。
 * 输入框内不触发（终端除外）。
 */
export function useKeyboard() {
  const actionCallbacks = useRef<Map<ShortcutActionId, () => void>>(new Map());

  const registerAction = useCallback((shortcutId: ShortcutActionId, action: () => void) => {
    actionCallbacks.current.set(shortcutId, action);
  }, []);

  const unregisterAction = useCallback((shortcutId: ShortcutActionId) => {
    actionCallbacks.current.delete(shortcutId);
  }, []);

  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      // 忽略输入框内的快捷键（但允许终端区域）
      if (isInputFocused()) {
        return;
      }

      const matched = shortcutRegistry.match(e);
      if (matched) {
        const action = actionCallbacks.current.get(matched.id);
        if (action) {
          e.preventDefault();
          e.stopPropagation();
          action();
        }
      }
    };

    document.addEventListener("keydown", handleKeyDown);
    return () => document.removeEventListener("keydown", handleKeyDown);
  }, []);

  return { registerAction, unregisterAction };
}
