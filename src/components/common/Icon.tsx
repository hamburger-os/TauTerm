import { type CSSProperties, type HTMLAttributes } from "react";
import styles from "./Icon.module.css";

// Every non-status icon is an independently produced 256px RGBA PNG. Keep the
// registry explicit: an absent asset or an unreviewed name becomes a type error.
import appearancePng from "../../assets/icons/appearance.png";
import arrowDownPng from "../../assets/icons/arrow-down.png";
import arrowLeftPng from "../../assets/icons/arrow-left.png";
import arrowRightPng from "../../assets/icons/arrow-right.png";
import arrowUpPng from "../../assets/icons/arrow-up.png";
import caretDownPng from "../../assets/icons/caret-down.png";
import chartPng from "../../assets/icons/chart.png";
import checkPng from "../../assets/icons/check.png";
import checkCirclePng from "../../assets/icons/check-circle.png";
import chevronDownPng from "../../assets/icons/chevron-down.png";
import chevronRightPng from "../../assets/icons/chevron-right.png";
import chevronUpPng from "../../assets/icons/chevron-up.png";
import clipboardPng from "../../assets/icons/clipboard.png";
import closePng from "../../assets/icons/close.png";
import codePng from "../../assets/icons/code.png";
import commandsPng from "../../assets/icons/commands.png";
import connectionPng from "../../assets/icons/connection.png";
import constructionPng from "../../assets/icons/construction.png";
import downloadPng from "../../assets/icons/download.png";
import dragHandlePng from "../../assets/icons/drag-handle.png";
import editPng from "../../assets/icons/edit.png";
import endpointPng from "../../assets/icons/endpoint.png";
import filePng from "../../assets/icons/file.png";
import folderPng from "../../assets/icons/folder.png";
import globePng from "../../assets/icons/globe.png";
import hourglassPng from "../../assets/icons/hourglass.png";
import infoPng from "../../assets/icons/info.png";
import keyboardPng from "../../assets/icons/keyboard.png";
import lockPng from "../../assets/icons/lock.png";
import logPng from "../../assets/icons/log.png";
import logoPng from "../../assets/icons/logo.png";
import loopPng from "../../assets/icons/loop.png";
import packagePng from "../../assets/icons/package.png";
import pastePng from "../../assets/icons/paste.png";
import playPng from "../../assets/icons/play.png";
import plusPng from "../../assets/icons/plus.png";
import refreshPng from "../../assets/icons/refresh.png";
import robotPng from "../../assets/icons/robot.png";
import searchPng from "../../assets/icons/search.png";
import sendPng from "../../assets/icons/send.png";
import settingsPng from "../../assets/icons/settings.png";
import sidebarLeftPng from "../../assets/icons/sidebar-left.png";
import sidebarRightPng from "../../assets/icons/sidebar-right.png";
import sshShellPng from "../../assets/icons/ssh-shell.png";
import statusCancelledPng from "../../assets/icons/status-cancelled.png";
import statusSkippedPng from "../../assets/icons/status-skipped.png";
import stepsPng from "../../assets/icons/steps.png";
import stopPng from "../../assets/icons/stop.png";
import stopwatchPng from "../../assets/icons/stopwatch.png";
import tagPng from "../../assets/icons/tag.png";
import transferActivePng from "../../assets/icons/transfer-active.png";
import trashPng from "../../assets/icons/trash.png";
import uploadPng from "../../assets/icons/upload.png";
import viewGridPng from "../../assets/icons/view-grid.png";
import viewListPng from "../../assets/icons/view-list.png";
import warningPng from "../../assets/icons/warning.png";
import windowClosePng from "../../assets/icons/window-close.png";
import windowMaximizePng from "../../assets/icons/window-maximize.png";
import windowMinimizePng from "../../assets/icons/window-minimize.png";
import windowRestorePng from "../../assets/icons/window-restore.png";
import xCirclePng from "../../assets/icons/x-circle.png";

const PNG_MAP = {
  appearance: appearancePng, "arrow-down": arrowDownPng, "arrow-left": arrowLeftPng,
  "arrow-right": arrowRightPng, "arrow-up": arrowUpPng, "caret-down": caretDownPng,
  chart: chartPng, check: checkPng, "check-circle": checkCirclePng,
  "chevron-down": chevronDownPng, "chevron-right": chevronRightPng, "chevron-up": chevronUpPng,
  clipboard: clipboardPng, close: closePng, code: codePng, commands: commandsPng,
  connection: connectionPng, construction: constructionPng, download: downloadPng,
  "drag-handle": dragHandlePng, edit: editPng, endpoint: endpointPng, file: filePng,
  folder: folderPng, globe: globePng, hourglass: hourglassPng, info: infoPng,
  keyboard: keyboardPng, lock: lockPng, log: logPng, logo: logoPng, loop: loopPng,
  package: packagePng, paste: pastePng, play: playPng, plus: plusPng, refresh: refreshPng,
  robot: robotPng, search: searchPng, send: sendPng, settings: settingsPng,
  "sidebar-left": sidebarLeftPng, "sidebar-right": sidebarRightPng, "ssh-shell": sshShellPng,
  "status-cancelled": statusCancelledPng, "status-skipped": statusSkippedPng, steps: stepsPng,
  stop: stopPng, stopwatch: stopwatchPng, tag: tagPng, "transfer-active": transferActivePng,
  trash: trashPng, upload: uploadPng, "view-grid": viewGridPng, "view-list": viewListPng,
  warning: warningPng, "window-close": windowClosePng, "window-maximize": windowMaximizePng,
  "window-minimize": windowMinimizePng, "window-restore": windowRestorePng, "x-circle": xCirclePng,
} satisfies Record<string, string>;

for (const pngUrl of Object.values(PNG_MAP)) new Image().src = pngUrl;

type PngIconName = keyof typeof PNG_MAP;
type StatusIconName = "status-connected" | "status-disconnected" | "status-connecting" | "status-transferring" | "status-idle";

/** All registered PNG assets plus the five CSS-only state dots. */
export type IconName = PngIconName | StatusIconName;

const SIZE_MAP: Record<string, number> = { xs: 12, sm: 14, md: 18, lg: 24, xl: 36, "2xl": 48 };

export interface IconProps extends Omit<HTMLAttributes<HTMLElement>, "color"> {
  name: IconName;
  size?: "xs" | "sm" | "md" | "lg" | "xl" | "2xl" | number;
  label?: string;
}

const STATUS_CLASS_MAP: Record<StatusIconName, string> = {
  "status-connected": styles.statusConnected,
  "status-disconnected": styles.statusDisconnected,
  "status-connecting": styles.statusConnecting,
  "status-transferring": styles.statusTransferring,
  "status-idle": styles.statusIdle,
};

function isStatusIconName(name: IconName): name is StatusIconName {
  return Object.prototype.hasOwnProperty.call(STATUS_CLASS_MAP, name);
}

/** Functional and window-control icons are always PNG; only state dots use CSS. */
export default function Icon({
  name, size = "md", className = "", label, style: externalStyle, ...spanProps
}: IconProps) {
  const sizePx = typeof size === "number" ? size : SIZE_MAP[size] || SIZE_MAP.md;
  const inlineStyle: CSSProperties = { width: sizePx, height: sizePx, ...externalStyle };

  if (isStatusIconName(name)) {
    return <span className={`${styles.statusDot} ${STATUS_CLASS_MAP[name]} ${className}`.trim()} style={inlineStyle} role={label ? "img" : "presentation"} aria-label={label} {...spanProps} />;
  }

  return <img src={PNG_MAP[name]} alt={label || ""} className={`${styles.imgIcon} ${className}`.trim()} style={inlineStyle} role={label ? "img" : "presentation"} {...spanProps} />;
}
