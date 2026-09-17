import { useCallback, useEffect, useRef } from "react";
import { shortcutRegistry } from "../shortcuts/registry";
import type { ShortcutActionId } from "../shortcuts/actionIds";
import { isInputFocused } from "../utils/dom";

type RegisteredAction = {
  owner: symbol;
  callback: () => void;
};

const actionCallbacks = new Map<ShortcutActionId, RegisteredAction>();
let keyboardListenerUsers = 0;

function executeRegisteredAction(actionId: ShortcutActionId): boolean {
  const action = actionCallbacks.get(actionId);
  if (!action) return false;
  action.callback();
  return true;
}

function handleGlobalKeyDown(event: KeyboardEvent): void {
  if (isInputFocused()) return;

  const matched = shortcutRegistry.match(event);
  if (!matched || !executeRegisteredAction(matched.id)) return;

  event.preventDefault();
  event.stopPropagation();
}

/**
 * 全局快捷键与动作分发 hook。
 *
 * Action ID 只注册到一份进程内 registry。键盘、命令面板和工具栏都通过同一
 * dispatcher 执行动作，避免各入口分别维护 switch/回调镜像。document 级键盘
 * listener 由所有 hook 实例共享，多个消费者不会重复执行同一快捷键。
 */
export function useKeyboard() {
  const ownerRef = useRef(Symbol("shortcut-action-owner"));
  const ownedActionsRef = useRef(new Set<ShortcutActionId>());

  const registerAction = useCallback((shortcutId: ShortcutActionId, action: () => void) => {
    actionCallbacks.set(shortcutId, { owner: ownerRef.current, callback: action });
    ownedActionsRef.current.add(shortcutId);
  }, []);

  const unregisterAction = useCallback((shortcutId: ShortcutActionId) => {
    const registered = actionCallbacks.get(shortcutId);
    if (registered?.owner === ownerRef.current) {
      actionCallbacks.delete(shortcutId);
    }
    ownedActionsRef.current.delete(shortcutId);
  }, []);

  const executeAction = useCallback((shortcutId: ShortcutActionId) => {
    return executeRegisteredAction(shortcutId);
  }, []);

  useEffect(() => {
    keyboardListenerUsers += 1;
    if (keyboardListenerUsers === 1) {
      document.addEventListener("keydown", handleGlobalKeyDown);
    }

    return () => {
      for (const shortcutId of ownedActionsRef.current) {
        const registered = actionCallbacks.get(shortcutId);
        if (registered?.owner === ownerRef.current) {
          actionCallbacks.delete(shortcutId);
        }
      }
      ownedActionsRef.current.clear();

      keyboardListenerUsers = Math.max(0, keyboardListenerUsers - 1);
      if (keyboardListenerUsers === 0) {
        document.removeEventListener("keydown", handleGlobalKeyDown);
      }
    };
  }, []);

  return { registerAction, unregisterAction, executeAction };
}
