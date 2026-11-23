import type { Terminal } from "@xterm/xterm";
import { isInputFocused } from "../../utils/dom";

/**
 * 终端键盘焦点的单一所有者。
 *
 * xterm 用一个隐藏的 textarea 收键盘事件，所以终端只有被显式聚焦才能收到按键。
 * 连接成功、切换标签、切换分屏之后如果没有聚焦，按键会落到宿主按钮上——
 * 用户看到的是"输出还在动，但终端完全没反应"。反过来，任何组件都不该在
 * 弹窗或真实输入框里抢焦点。
 *
 * 因此"何时允许终端拿走焦点"只由本模块回答，surface 只负责提供自己的实例。
 */

type TerminalResolver = () => Terminal | null;

function hasOpenModal(): boolean {
  // 覆盖三类“用户正在和叠加层交互”的状态：模态对话框（`aria-modal`）、
  // 普通 dialog 语义，以及打开中的右键菜单。只要其中任一存在，激活终端
  // 也不能把焦点抢走。
  return document.querySelector(
    '[aria-modal="true"], [role="dialog"], [role="alertdialog"], [role="menu"]',
  ) !== null;
}

/**
 * 激活类场景（连接完成、切换标签、切换分屏）是否允许聚焦终端。
 *
 * 弹窗打开时不抢；真实输入框（弹窗输入、SendBar 输入框、搜索框）有焦点时也不抢。
 * xterm 自己的 textarea 不算"真实输入框"（`isInputFocused` 已把它排除）。
 */
function canClaimFocus(): boolean {
  if (hasOpenModal()) return false;
  return !isInputFocused();
}

/** 与用户当前输入无关的激活：只在不会抢走输入时聚焦。 */
export function focusTerminalOnActivation(resolve: TerminalResolver): void {
  requestAnimationFrame(() => {
    if (!canClaimFocus()) return;
    resolve()?.focus();
  });
}

/**
 * 终端内部操作（复制/粘贴/搜索/右键菜单关闭）之后把焦点还给终端。
 *
 * 这些调用点常常位于一次异步剪贴板 IPC 之后：等待期间用户可能已经打开了弹窗
 * 或点进了真实输入框，所以“归还”同样要经过 `canClaimFocus()`，不能无条件聚焦。
 */
export function restoreTerminalFocus(resolve: TerminalResolver): void {
  requestAnimationFrame(() => {
    if (!canClaimFocus()) return;
    resolve()?.focus();
  });
}
