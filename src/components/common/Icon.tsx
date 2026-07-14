import { type HTMLAttributes } from "react";
import styles from "./Icon.module.css";

// ── Tier 1: PNG mask-image imports ────────────────────────────
// 待用户用 AI 生成真实图标后替换这些占位文件
import logoPng from "../../assets/icons/logo.png";
import zmodemPng from "../../assets/icons/zmodem.png";
import plugPng from "../../assets/icons/plug.png";
import pinPng from "../../assets/icons/pin.png";
import tagPng from "../../assets/icons/tag.png";
import settingsPng from "../../assets/icons/settings.png";
import palettePng from "../../assets/icons/palette.png";
import panelRightPng from "../../assets/icons/panel-right.png";
import globePng from "../../assets/icons/globe.png";
import fontPng from "../../assets/icons/font.png";
import infoPng from "../../assets/icons/info.png";
import searchPng from "../../assets/icons/search.png";
import uploadPng from "../../assets/icons/upload.png";
import downloadPng from "../../assets/icons/download.png";
import packagePng from "../../assets/icons/package.png";
import antennaPng from "../../assets/icons/antenna.png";
import trashPng from "../../assets/icons/trash.png";
import stopPng from "../../assets/icons/stop.png";
import playPng from "../../assets/icons/play.png";
import constructionPng from "../../assets/icons/construction.png";
import folderPng from "../../assets/icons/folder.png";
import chartPng from "../../assets/icons/chart.png";
import warningPng from "../../assets/icons/warning.png";
import stopwatchPng from "../../assets/icons/stopwatch.png";
import checkCirclePng from "../../assets/icons/check-circle.png";
import crossCirclePng from "../../assets/icons/cross-circle.png";
import skipPng from "../../assets/icons/skip.png";
import hourglassPng from "../../assets/icons/hourglass.png";
import transferProgressPng from "../../assets/icons/transfer-progress.png";
import checkPlainPng from "../../assets/icons/check-plain.png";
import logPng from "../../assets/icons/log.png";
import dragHandlePng from "../../assets/icons/drag-handle.png";
import editPng from "../../assets/icons/edit.png";
import loopPng from "../../assets/icons/loop.png";
import closePng from "../../assets/icons/close.png";
import menuPng from "../../assets/icons/menu.png";
import chevronUpPng from "../../assets/icons/chevron-up.png";
import chevronDownPng from "../../assets/icons/chevron-down.png";
import chevronDropdownPng from "../../assets/icons/chevron-dropdown.png";
import refreshPng from "../../assets/icons/refresh.png";
import plusPng from "../../assets/icons/plus.png";
import backArrowPng from "../../assets/icons/back-arrow.png";
import commandPanelPng from "../../assets/icons/command-panel.png";
import keyboardPng from "../../assets/icons/keyboard.png";
import robotPng from "../../assets/icons/robot.png";
import codePng from "../../assets/icons/code.png";
import chevronRightPng from "../../assets/icons/chevron-right.png";

// ── PNG URL Mapping ───────────────────────────────────────────
// Must be defined before IconName type so PNG key list can be derived

const PNG_MAP: Record<string, string> = {
  logo: logoPng,
  zmodem: zmodemPng,
  plug: plugPng,
  pin: pinPng,
  tag: tagPng,
  settings: settingsPng,
  palette: palettePng,
  "panel-right": panelRightPng,
  globe: globePng,
  font: fontPng,
  info: infoPng,
  search: searchPng,
  upload: uploadPng,
  download: downloadPng,
  package: packagePng,
  antenna: antennaPng,
  trash: trashPng,
  stop: stopPng,
  play: playPng,
  construction: constructionPng,
  folder: folderPng,
  chart: chartPng,
  warning: warningPng,
  stopwatch: stopwatchPng,
  "check-circle": checkCirclePng,
  "cross-circle": crossCirclePng,
  skip: skipPng,
  hourglass: hourglassPng,
  "transfer-progress": transferProgressPng,
  "check-plain": checkPlainPng,
  log: logPng,
  "drag-handle": dragHandlePng,
  edit: editPng,
  loop: loopPng,
  close: closePng,
  menu: menuPng,
  "chevron-up": chevronUpPng,
  "chevron-down": chevronDownPng,
  "chevron-dropdown": chevronDropdownPng,
  refresh: refreshPng,
  plus: plusPng,
  "back-arrow": backArrowPng,
  "command-panel": commandPanelPng,
  keyboard: keyboardPng,
  robot: robotPng,
  code: codePng,
  "chevron-right": chevronRightPng,
};

// ── Preload: 模块加载时提前下载所有 47 个 PNG 图标到浏览器缓存 ──
// 使用 new Image() 在 JS 解析阶段立即发起下载，避免 React 渲染 <img>
// 标签后才开始请求导致的"弹入"延迟。到渲染时图片已在缓存中，即时显示。
for (const pngUrl of Object.values(PNG_MAP)) {
  new Image().src = pngUrl;
}

// ── Type Definitions ──────────────────────────────────────────

/** 从 PNG_MAP 的键推导，避免与常量集不同步 */
type PngIconName = keyof typeof PNG_MAP;

/** 所有图标名称的联合类型 */
export type IconName =
  // Tier 1: PNG mask-image (keyof PNG_MAP → 47 icons)
  | PngIconName
  // Tier 2: CSS status dots (4 icons)
  | "status-connected"
  | "status-disconnected"
  | "status-connecting"
  | "status-idle"
  // Tier 3: Inline SVG — title bar window controls only (3 icons)
  | "window-minimize"
  | "window-maximize"
  | "window-restore";

/** 预设尺寸映射到 CSS 像素值 */
const SIZE_MAP: Record<string, number> = {
  xs: 12,
  sm: 14,
  md: 18,
  lg: 24,
  xl: 36,
  "2xl": 48,
};

export interface IconProps extends HTMLAttributes<HTMLElement> {
  /** 图标名称 */
  name: IconName;
  /**
   * 图标尺寸
   * - 预设: "xs" | "sm" | "md" | "lg" | "xl" | "2xl"
   * - 自定义: 数字（像素）
   * @default "md"
   */
  size?: "xs" | "sm" | "md" | "lg" | "xl" | "2xl" | number;
  /**
   * 覆盖图标颜色（CSS 颜色值）
   * 默认: currentColor（继承父元素文字颜色，自动适配主题）
   */
  color?: string;
  /** 无障碍标签（用于装饰性图标时为屏幕阅读器提供文本） */
  label?: string;
}

// ── Status Dot Class Mapping ──────────────────────────────────

const STATUS_CLASS_MAP: Record<string, string> = {
  "status-connected": styles.statusConnected,
  "status-disconnected": styles.statusDisconnected,
  "status-connecting": styles.statusConnecting,
  "status-idle": styles.statusIdle,
};

// ── Component ─────────────────────────────────────────────────

/**
 * 统一图标组件
 *
 * 三种内部渲染策略：
 * - Tier 1 (47 个): CSS mask-image / <img> 渲染 PNG，通过 currentColor 自动适配主题
 * - Tier 2 (4 个):  纯 CSS 状态圆点 + 主题色发光
 * - Tier 3 (3 个):  内联 SVG（仅顶栏窗口控制按钮）
 *
 * @example
 * <Icon name="settings" size="sm" />
 * <Icon name="warning" size="lg" color="var(--color-warning)" />
 * <Icon name="status-connected" size="xs" />
 * <Icon name="close" size="sm" label="关闭" />
 */
export default function Icon({
  name,
  size = "md",
  color,
  className = "",
  label,
  style: externalStyle,
  ...spanProps
}: IconProps) {
  // 计算尺寸
  const sizePx =
    typeof size === "number" ? size : SIZE_MAP[size] || SIZE_MAP.md;

  const inlineStyle: React.CSSProperties = {
    width: sizePx,
    height: sizePx,
    ...(color ? { color } : {}),
    ...externalStyle,
  };

  // ── Tier 2: CSS Status Dots ──────────────────────────────
  if (name in STATUS_CLASS_MAP) {
    const dotClass = STATUS_CLASS_MAP[name];
    return (
      <span
        className={`${styles.statusDot} ${dotClass} ${className}`.trim()}
        style={inlineStyle}
        role={label ? "img" : "presentation"}
        aria-label={label}
        {...spanProps}
      />
    );
  }

  // ── Tier 1: PNG Mask-Image ──────────────────────────────
  if (name in PNG_MAP) {
    const pngUrl = PNG_MAP[name];
    // 显式传入 color 时使用 mask-image（主题自适应着色）
    if (color) {
      return (
        <span
          className={`${styles.maskIcon} ${className}`.trim()}
          style={{
            ...inlineStyle,
            maskImage: `url(${pngUrl})`,
            WebkitMaskImage: `url(${pngUrl})`,
            backgroundColor: color,
          }}
          role={label ? "img" : "presentation"}
          aria-label={label}
          {...spanProps}
        />
      );
    }
    // 默认渲染 <img> 保留原始 PNG 的玻璃质感视觉效果
    return (
      <img
        src={pngUrl}
        alt={label || ""}
        className={`${styles.imgIcon} ${className}`.trim()}
        style={inlineStyle}
        role={label ? "img" : "presentation"}
        {...spanProps}
      />
    );
  }

  // ── Tier 3: Inline SVG ──────────────────────────────────
  // Note: spanProps (HTMLAttributes<HTMLSpanElement>) 不能传播到 SVG 元素
  const svgViewBox = "0 0 24 24";
  const svgBaseProps = {
    width: sizePx,
    height: sizePx,
    viewBox: svgViewBox,
    fill: "none" as const,
    stroke: "currentColor" as const,
    strokeWidth: "2.5" as const,
    strokeLinecap: "round" as const,
    strokeLinejoin: "round" as const,
    className,
    style: inlineStyle,
    role: label ? "img" as const : "presentation" as const,
    "aria-label": label || undefined,
  };

  switch (name) {
    case "window-minimize":
      return (
        <svg {...svgBaseProps} strokeWidth="2">
          <line x1="5" y1="19" x2="19" y2="19" />
        </svg>
      );

    case "window-maximize":
      return (
        <svg {...svgBaseProps} strokeWidth="2">
          <rect x="5" y="5" width="14" height="14" rx="1" />
        </svg>
      );

    case "window-restore":
      return (
        <svg {...svgBaseProps} strokeWidth="2">
          <rect x="4" y="7" width="12" height="12" rx="1" />
          <rect x="8" y="3" width="12" height="12" rx="1" />
        </svg>
      );

    default:
      // Fallback: 未知图标渲染为空白占位
      return (
        <span
          className={`${styles.maskIcon} ${className}`.trim()}
          style={inlineStyle}
          role="presentation"
          {...spanProps}
        />
      );
  }
}
