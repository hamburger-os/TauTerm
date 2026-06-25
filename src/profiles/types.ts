import type { TabInfo } from "../context/SessionContext";
import type { IconName } from "../components/common/Icon";

/** 单个信息行 */
export interface ProfileItem {
  /** i18n 键 */
  label: string;
  /** 显示值 */
  value: string;
  /** 可选的图标名 */
  icon?: IconName;
  /** 值是否使用等宽字体 */
  monospace?: boolean;
}

/** 会话 Profile 数据 */
export interface SessionProfile {
  /** 左栏：身份信息 */
  identity: ProfileItem[];
  /** 右栏上部：协议参数 */
  parameters: ProfileItem[];
}

/** Profile 解析器：将 TabInfo 转换为 SessionProfile */
export type ProfileResolver = (tab: TabInfo) => SessionProfile;
