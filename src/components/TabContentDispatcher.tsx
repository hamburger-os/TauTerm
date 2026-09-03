/**
 * 主内容区调度器。
 *
 * Pane 决定多个 Session Content 的摆放，selected Pane 继续通过
 * SessionContext.activeTabId 驱动 SendBar / RightSidebar。
 * SplitLayoutContext 会在本地保存最后一次 Workspace 布局，并在下次启动时恢复；
 * Session 连接本身不会自动恢复。
 */
import SplitView from "./Layout/SplitView";
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
    <SplitView
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
