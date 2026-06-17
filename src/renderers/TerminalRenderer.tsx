/**
 * 终端渲染器
 *
 * 委托给 `components/Terminal/TerminalView`，后者管理 xterm.js 实例池。
 * 所有已连接的终端通过 CSS opacity 切换可见性，无需重建实例。
 */

import TerminalView from "../components/Terminal/TerminalView";

/**
 * 终端内容渲染器
 *
 * 渲染所有已连接标签页的 TerminalView（串口终端）。
 * TabContentDispatcher 根据活跃标签页插件的 content_type="terminal" 调度至此。
 */
export default function TerminalRenderer() {
  return <TerminalView />;
}
