import { memo } from "react";
import { motion, AnimatePresence } from "framer-motion";
import { useTranslation } from "react-i18next";
import Icon from "../common/Icon";
import styles from "./ScrollToBottomButton.module.css";

interface ScrollToBottomButtonProps {
  /** 按钮是否可见（用户不在底部时显示） */
  visible: boolean;
  /** 点击回调：父组件负责滚动到底部并重置 auto-scroll 状态 */
  onClick: () => void;
}

/**
 * 浮动"回到底部"按钮
 *
 * 用于终端（text/hex/dual 三种模式）的滚动容器。
 * 当用户向上滚动离开底部时浮现，点击后滚动回底部并恢复自动滚动。
 * 使用 framer-motion AnimatePresence 实现进出动画。
 * 使用 React.memo 避免因父组件重渲染（如终端数据流入）导致的不必要渲染。
 */
const ScrollToBottomButton = memo(function ScrollToBottomButton({ visible, onClick }: ScrollToBottomButtonProps) {
  const { t } = useTranslation();
  return (
    <AnimatePresence>
      {visible && (
        <motion.button
          className={`${styles.button} liquid-glass-float`}
          onClick={onClick}
          initial={{ opacity: 0, scale: 0.8, y: 8 }}
          animate={{ opacity: 1, scale: 1, y: 0 }}
          exit={{ opacity: 0, scale: 0.8, y: 8 }}
          transition={{ duration: 0.2, ease: "easeOut" }}
          aria-label={t("terminal.scrollToBottom")}
        >
          <Icon name="chevron-dropdown" size="sm" />
        </motion.button>
      )}
    </AnimatePresence>
  );
});

export default ScrollToBottomButton;
