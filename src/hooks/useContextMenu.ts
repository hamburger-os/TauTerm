import { useState, useCallback } from "react";
import type { TabInfo } from "../context/SessionContext";

export interface ContextMenuState {
  x: number;
  y: number;
  visible: boolean;
  session: TabInfo | null;
  /** 插件贡献的 Session 树后台实体；公共菜单层只保留不透明标识。 */
  extension?: { parentSessionId: string; childId: string } | null;
}

export function useContextMenu() {
  const [menu, setMenu] = useState<ContextMenuState>({
    x: 0,
    y: 0,
    visible: false,
    session: null,
    extension: null,
  });

  const openMenu = useCallback((e: React.MouseEvent, session: TabInfo) => {
    e.preventDefault();
    setMenu({
      x: e.clientX,
      y: e.clientY,
      visible: true,
      session,
      extension: null,
    });
  }, []);

  const openExtensionMenu = useCallback((
    e: React.MouseEvent,
    session: TabInfo,
    parentSessionId: string,
    childId: string,
  ) => {
    e.preventDefault();
    setMenu({
      x: e.clientX,
      y: e.clientY,
      visible: true,
      session,
      extension: { parentSessionId, childId },
    });
  }, []);

  const closeMenu = useCallback(() => {
    setMenu(prev => ({ ...prev, visible: false }));
  }, []);

  return { menu, openMenu, openExtensionMenu, closeMenu };
}
