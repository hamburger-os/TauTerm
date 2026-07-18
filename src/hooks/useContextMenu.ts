import { useState, useCallback, useEffect } from "react";
import type { TabInfo } from "../context/SessionContext";

export interface ContextMenuState {
  /** 屏幕坐标 X（左上角原点） */
  x: number;
  /** 屏幕坐标 Y */
  y: number;
  /** 菜单是否可见 */
  visible: boolean;
  /** 右键目标会话（null = 无目标，菜单不可见） */
  session: TabInfo | null;
}

/**
 * 右键上下文菜单 Hook
 *
 * 管理菜单位置、可见性和目标会话。
 * 点击外部或按 Escape 自动关闭。
 */
export function useContextMenu() {
  const [menu, setMenu] = useState<ContextMenuState>({
    x: 0,
    y: 0,
    visible: false,
    session: null,
  });

  const openMenu = useCallback((e: React.MouseEvent, session: TabInfo) => {
    e.preventDefault();
    setMenu({
      x: e.clientX,
      y: e.clientY,
      visible: true,
      session,
    });
  }, []);

  const closeMenu = useCallback(() => {
    setMenu(prev => ({ ...prev, visible: false }));
  }, []);

  // 点击外部关闭
  useEffect(() => {
    if (!menu.visible) return;
    const handleClick = () => closeMenu();
    const handleKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") closeMenu();
    };
    // 延迟添加监听，避免触发右键的同一次点击也关闭菜单
    const timer = setTimeout(() => {
      document.addEventListener("click", handleClick);
      document.addEventListener("keydown", handleKey);
    }, 0);
    return () => {
      clearTimeout(timer);
      document.removeEventListener("click", handleClick);
      document.removeEventListener("keydown", handleKey);
    };
  }, [menu.visible, closeMenu]);

  return { menu, openMenu, closeMenu };
}
