import { type ButtonHTMLAttributes } from "react";
import Icon from "./Icon";
import styles from "../Settings/SettingsPage.module.css";

interface OptionButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  /** 当前选项是否被选中（显示 check 图标） */
  selected: boolean;
}

/**
 * Settings 面板选项按钮
 *
 * 封装了 check 图标 + glass-button 样式 + active 态切换的重复模板。
 * 图标始终渲染以保持布局稳定，仅通过 visibility 控制可见性。
 */
export default function OptionButton({
  selected,
  children,
  className = "",
  ...buttonProps
}: OptionButtonProps) {
  return (
    <button
      className={`${styles.optionBtn} liquid-glass-button ${selected ? "active" : ""} ${className}`.trim()}
      {...buttonProps}
    >
      <Icon
        name="check"
        size="sm"
        style={{ visibility: selected ? "visible" : "hidden" }}
      />
      {children}
    </button>
  );
}
