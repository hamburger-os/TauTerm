/**
 * 主内容区调度器。
 *
 * 分屏只是一层运行时布局：Pane 决定多个 Session Content 的摆放，
 * selected Pane 继续通过 SessionContext.activeTabId 驱动 SendBar / RightSidebar。
 * 不保存布局，也不恢复上一次分屏。
 */
import SplitWorkspace from "./Layout/SplitWorkspace";
import { useSplitLayout } from "../context/SplitLayoutContext";

export default function TabContentDispatcher() {
  const {
    state,
    paneRects,
    dividers,
    blockedEdges,
    paneCount,
    selectPane,
    splitPane,
    closePane,
    resizeSplit,
  } = useSplitLayout();

  return (
    <SplitWorkspace
      layout={state}
      paneRects={paneRects}
      dividers={dividers}
      blockedEdges={blockedEdges}
      paneCount={paneCount}
      onSelectPane={selectPane}
      onSplitPane={splitPane}
      onClosePane={closePane}
      onResizeSplit={resizeSplit}
    />
  );
}
