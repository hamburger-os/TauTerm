import { useState, useCallback } from "react";
import { motion } from "framer-motion";
import styles from "./ResizeHandle.module.css";

interface ResizeHandleProps {
  direction: "horizontal" | "vertical";
  onMouseDown: (e: React.MouseEvent) => void;
  className?: string;
}

/**
 * 可拖拽分割线组件
 *
 * 鼠标接近 10px 范围时显示发光效果和胶囊图标。
 */
export default function ResizeHandle({ direction, onMouseDown, className }: ResizeHandleProps) {
  const [isNear, setIsNear] = useState(false);
  const [isActive, setIsActive] = useState(false);

  const isHorizontal = direction === "horizontal";

  const handleMouseMove = useCallback((e: React.MouseEvent) => {
    const rect = e.currentTarget.getBoundingClientRect();
    const distance = isHorizontal
      ? Math.min(
          Math.abs(e.clientX - rect.left),
          Math.abs(e.clientX - rect.right)
        )
      : Math.min(
          Math.abs(e.clientY - rect.top),
          Math.abs(e.clientY - rect.bottom)
        );
    setIsNear(distance < 20);
  }, [isHorizontal]);

  const handleMouseLeave = useCallback(() => {
    setIsNear(false);
  }, []);

  const handleMouseDown = useCallback((e: React.MouseEvent) => {
    setIsActive(true);
    onMouseDown(e);
    // Reset after mouse up
    const handleUp = () => {
      setIsActive(false);
      document.removeEventListener("mouseup", handleUp);
    };
    document.addEventListener("mouseup", handleUp);
  }, [onMouseDown]);

  return (
    <div
      className={`${styles.handle} ${isHorizontal ? styles.horizontal : styles.vertical} ${className || ""}`}
      onMouseMove={handleMouseMove}
      onMouseLeave={handleMouseLeave}
      onMouseDown={handleMouseDown}
    >
      <motion.div
        className={styles.glow}
        animate={{
          opacity: isNear || isActive ? 1 : 0,
          scaleX: isActive ? 1.05 : 1,
          scaleY: isActive ? 1.05 : 1,
        }}
        transition={{ duration: 0.2 }}
      />
      <motion.div
        className={styles.icon}
        animate={{
          opacity: isNear ? 1 : 0,
          scale: isActive ? 1.2 : 1,
        }}
        transition={{ duration: 0.2 }}
      >
        {isHorizontal ? "⋮" : "⋯"}
      </motion.div>
    </div>
  );
}
