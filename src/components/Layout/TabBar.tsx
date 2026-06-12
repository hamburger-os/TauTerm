import { useCallback } from "react";
import { motion, AnimatePresence, Reorder } from "framer-motion";
import { useSession } from "../../context/SessionContext";
import styles from "./TabBar.module.css";

interface TabBarProps {
  onNewSession?: () => void;
}

/**
 * 标签页栏组件
 *
 * 支持标签页拖拽排序、点击切换、关闭按钮。
 */
export default function TabBar({ onNewSession }: TabBarProps) {
  const { state, switchTab, disconnect, reorderTabs } = useSession();

  const handleClose = useCallback((e: React.MouseEvent, id: string) => {
    e.stopPropagation();
    if (state.tabs.length === 1) {
      // 关闭最后一个标签页时只是断开，不创建新标签
      disconnect(id);
    } else {
      disconnect(id);
    }
  }, [disconnect, state.tabs.length]);

  const handleReorder = useCallback((newOrder: string[]) => {
    reorderTabs(newOrder);
  }, [reorderTabs]);

  // 提取 tab IDs 用于 Reorder
  const tabIds = state.tabs.map(t => t.id);

  return (
    <div className={styles.tabBar}>
      <Reorder.Group
        axis="x"
        values={tabIds}
        onReorder={handleReorder}
        className={styles.tabList}
      >
        <AnimatePresence mode="popLayout">
          {state.tabs.map(tab => (
            <Reorder.Item
              key={tab.id}
              value={tab.id}
              as="div"
              className={`${styles.tab} ${state.activeTabId === tab.id ? styles.active : ""}`}
              onClick={() => switchTab(tab.id)}
              whileHover={{ backgroundColor: "rgba(255,255,255,0.06)" }}
              whileTap={{ scale: 0.97 }}
              initial={{ opacity: 0, x: -20 }}
              animate={{ opacity: 1, x: 0 }}
              exit={{ opacity: 0, x: -20, transition: { duration: 0.15 } }}
              layout
            >
              <span className={`${styles.tabDot} ${tab.state === "connected" ? styles.connected : ""}`} />
              <span className={styles.tabName}>{tab.name}</span>
              <motion.button
                className={styles.closeBtn}
                onClick={(e: React.MouseEvent) => handleClose(e, tab.id)}
                whileHover={{ backgroundColor: "rgba(255,255,255,0.15)", scale: 1.1 }}
                whileTap={{ scale: 0.9 }}
              >
                ×
              </motion.button>
            </Reorder.Item>
          ))}
        </AnimatePresence>
      </Reorder.Group>
      <motion.button
        className={styles.addBtn}
        onClick={onNewSession}
        whileHover={{ backgroundColor: "rgba(255,255,255,0.1)" }}
      >
        +
      </motion.button>
    </div>
  );
}
