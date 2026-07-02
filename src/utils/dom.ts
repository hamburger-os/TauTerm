/**
 * 共享 DOM 工具函数
 */

/** 检测当前焦点是否在输入框内（INPUT / TEXTAREA / contentEditable） */
export function isInputFocused(): boolean {
  const el = document.activeElement;
  if (!el) return false;
  const tag = (el as HTMLElement).tagName;
  return tag === "INPUT" || tag === "TEXTAREA" || (el as HTMLElement).isContentEditable;
}
