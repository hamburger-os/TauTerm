import { useEffect, useCallback, useRef } from "react";
import { shortcutRegistry } from "../shortcuts/registry";

/**
 * 全局快捷键监听 hook
 *
 * 在 document 级别监听键盘事件，匹配注册的快捷键并执行对应 action。
 * 输入框内不触发（终端除外）。
 */
export function useKeyboard() {
  const actionCallbacks = useRef<Map<string, () => void>>(new Map());

  const registerAction = useCallback((shortcutId: string, action: () => void) => {
    actionCallbacks.current.set(shortcutId, action);
  }, []);

  const unregisterAction = useCallback((shortcutId: string) => {
    actionCallbacks.current.delete(shortcutId);
  }, []);

  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      // 忽略输入框内的快捷键（但允许终端区域）
      const target = e.target as HTMLElement;
      if (
        target.tagName === "INPUT" ||
        target.tagName === "TEXTAREA" ||
        target.isContentEditable
      ) {
        // 如果目标是搜索栏等特定输入，允许部分快捷键通过
        // Ctrl+F 在当前无搜索栏时触发搜索
        return;
      }

      const matched = shortcutRegistry.match(e);
      if (matched) {
        e.preventDefault();
        e.stopPropagation();
        const action = actionCallbacks.current.get(matched.id);
        if (action) {
          action();
        }
      }
    };

    document.addEventListener("keydown", handleKeyDown);
    return () => document.removeEventListener("keydown", handleKeyDown);
  }, []);

  return { registerAction, unregisterAction };
}
