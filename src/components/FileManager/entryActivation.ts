/**
 * 目录项激活与符号链接解析
 *
 * 双击 / Enter / “打开”的用户语义，以及符号链接“跟随一次”的规则只在这里实现：
 * - 目录：导航逻辑路径 `entry.path`；
 * - 普通文件：直接进入 Remote Document；
 * - 符号链接：调用 `sftp_resolve_entry_cmd` 解析一次，目标为目录时仍导航逻辑路径，
 *   目标为文件时打开文档，目标是特殊文件或链接已断时给出提示；
 * - 特殊文件：没有可执行的激活语义。
 *
 * 下载分类、复制链接目标、打开链接目标位置复用同一份解析结果。UI 组件只提供
 * 导航 / 打开 / 提示回调，不重复解析和类型分派。
 */
import { invoke } from "@tauri-apps/api/core";
import { sftpErrorCode, sftpErrorMessage } from "../../services/sftpErrors";
import type { SftpErrorCode } from "../../services/sftpErrors";
import type { SftpEntry, SftpEntryType } from "./types";
import {
  isDirectory,
  isRegularFile,
  isSpecial,
  isSymlink,
} from "./entryKind";

/** 链接解析错误类别；与后端 `SftpErrorCode`（含 `loop`）保持一致。 */
export type EntryResolutionErrorCode = SftpErrorCode;

/**
 * `sftp_resolve_entry_cmd` 的返回。
 *
 * 目标不可用不是 IPC 错误：`target_exists == false` 时用 `resolution_error_code`
 * 区分断链、无权限、循环等语义，UI 据此给出不同提示。
 */
export interface ResolvedEntry {
  /** 传入的原始路径（逻辑路径身份）。 */
  path: string;
  /** 原始路径自身的 lstat 类型。 */
  entry_type: SftpEntryType;
  /** `READLINK` 返回的目标原文（可能是相对路径）；非链接时为 null。 */
  link_target: string | null;
  /** 目标的绝对路径：优先 REALPATH，不可用时按链接目录拼接目标原文。 */
  target_path: string | null;
  /** 跟随后的目标类型；目标不可解析时为 null。 */
  target_type: SftpEntryType | null;
  /** 跟随 STAT 是否成功。 */
  target_exists: boolean;
  /** 跟随后的目标大小（目录 / 特殊文件为 null）。 */
  target_size: number | null;
  /** 目标不可解析时的错误类别。 */
  resolution_error_code: EntryResolutionErrorCode | null;
  /** 目标不可解析时的用户可见说明。 */
  resolution_error: string | null;
}

/** 激活操作需要的副作用回调；解析与类型分派保持在模块内部。 */
export interface EntryActivationHandlers {
  /** 导航到某个远端目录（调用方负责清空选择等 UI 状态）。 */
  navigate: (path: string) => void;
  /** 用条目打开 Remote Document。 */
  openDocument: (entry: SftpEntry) => void;
  /** 以警告提示一个 i18n key。 */
  notify: (messageKey: string) => void;
}

/** 解析后的目标类型分派结果。 */
export type ResolutionOutcome =
  | { type: "directory" }
  | { type: "file" }
  | { type: "notice"; messageKey: string };

/** 调用后端解析一条目录项（符号链接会额外给出目标信息）。 */
export function resolveEntry(sessionId: string, remotePath: string): Promise<ResolvedEntry> {
  return invoke<ResolvedEntry>("sftp_resolve_entry_cmd", {
    sessionId,
    remotePath,
  });
}

/** 链接目标不可用时的提示 key。 */
export function resolutionErrorKey(
  code: EntryResolutionErrorCode | null,
): string {
  switch (code) {
    case "not_found":
      return "fileManager.linkTargetMissing";
    case "permission_denied":
      return "fileManager.linkTargetPermissionDenied";
    case "unsupported":
      return "fileManager.linkTargetUnsupported";
    case "loop":
      return "fileManager.linkTargetTooDeep";
    default:
      return "fileManager.linkTargetUnresolvable";
  }
}

/**
 * 把解析结果映射成“可跟随的目标类型”或“需要提示的原因”，不产生副作用。
 * 目标不可用优先按错误类别提示；目标存在但是特殊文件时单独提示。
 */
export function resolutionOutcome(resolution: ResolvedEntry): ResolutionOutcome {
  if (!resolution.target_exists || resolution.target_type === null) {
    return {
      type: "notice",
      messageKey: resolutionErrorKey(resolution.resolution_error_code),
    };
  }
  const target = { entry_type: resolution.target_type };
  if (isDirectory(target)) return { type: "directory" };
  if (isRegularFile(target)) return { type: "file" };
  return { type: "notice", messageKey: "fileManager.linkTargetSpecial" };
}

/**
 * 属性弹窗“目标状态”对应的 i18n key。
 * 目标可正常跟随时返回 null，由调用方展示目标类型标签。
 */
export function resolutionStatusKey(resolution: ResolvedEntry): string | null {
  if (!resolution.target_exists || resolution.target_type === null) {
    return resolutionErrorKey(resolution.resolution_error_code);
  }
  const target = { entry_type: resolution.target_type };
  if (isDirectory(target) || isRegularFile(target)) return null;
  return "fileManager.linkTargetSpecial";
}

/** 解析命令本身失败时的占位结果，让属性弹窗仍能展示“目标不可解析”。 */
export function resolutionFailure(remotePath: string, error: unknown): ResolvedEntry {
  return {
    path: remotePath,
    entry_type: "symlink",
    link_target: null,
    target_path: null,
    target_type: null,
    target_exists: false,
    target_size: null,
    resolution_error_code: sftpErrorCode(error),
    resolution_error: sftpErrorMessage(error),
  };
}

/**
 * 执行一次用户激活。
 *
 * 符号链接只跟随一次：目标为目录时导航链接自身的逻辑路径（不是目标真实路径），
 * 目标为文件时用链接条目打开文档（文档读取结果会带回真实路径用于保存 pinning）。
 */
export async function activateEntry(
  sessionId: string,
  entry: SftpEntry,
  handlers: EntryActivationHandlers,
): Promise<void> {
  if (isDirectory(entry)) {
    handlers.navigate(entry.path);
    return;
  }
  if (isRegularFile(entry)) {
    handlers.openDocument(entry);
    return;
  }
  if (isSpecial(entry)) {
    // 特殊文件没有可执行的激活语义：双击/回车必须给出原因，而不是静默无反应。
    handlers.notify("fileManager.specialFileCannotOpen");
    return;
  }

  let resolution: ResolvedEntry;
  try {
    resolution = await resolveEntry(sessionId, entry.path);
  } catch (error) {
    handlers.notify(resolutionErrorKey(sftpErrorCode(error)));
    return;
  }

  const outcome = resolutionOutcome(resolution);
  if (outcome.type === "notice") {
    handlers.notify(outcome.messageKey);
    return;
  }
  if (outcome.type === "directory") {
    handlers.navigate(entry.path);
    return;
  }
  handlers.openDocument(entry);
}

/** 远端路径的父目录；根目录的父目录仍是根目录。 */
export function parentRemotePath(remotePath: string): string {
  const normalized = remotePath.replace(/\/+$/, "");
  if (normalized === "" || normalized === "/") return "/";
  const index = normalized.lastIndexOf("/");
  return index <= 0 ? "/" : normalized.slice(0, index);
}

/**
 * 打开“链接目标位置”。
 *
 * 目标是目录时导航到目标真实路径本身；目标是文件 / 特殊文件时导航到目标所在目录，
 * 因为列表只能打开目录。链接已断时按错误类别提示。
 */
export async function openLinkTargetLocation(
  sessionId: string,
  entry: SftpEntry,
  handlers: EntryActivationHandlers,
): Promise<void> {
  if (!isSymlink(entry)) return;
  let resolution: ResolvedEntry;
  try {
    resolution = await resolveEntry(sessionId, entry.path);
  } catch (error) {
    handlers.notify(resolutionErrorKey(sftpErrorCode(error)));
    return;
  }

  if (!resolution.target_exists || resolution.target_path === null) {
    handlers.notify(resolutionErrorKey(resolution.resolution_error_code));
    return;
  }
  const targetType = resolution.target_type;
  const targetIsDirectory = targetType !== null
    && isDirectory({ entry_type: targetType });
  handlers.navigate(
    targetIsDirectory
      ? resolution.target_path
      : parentRemotePath(resolution.target_path),
  );
}

/** 复制链接目标需要的文本；链接已断时返回提示 key。 */
export interface LinkTargetText {
  text: string | null;
  messageKey: string | null;
}

/** 读取可复制的链接目标文本（目标原文优先，其次真实路径）。 */
export async function readLinkTargetText(
  sessionId: string,
  entry: SftpEntry,
): Promise<LinkTargetText> {
  if (!isSymlink(entry)) return { text: null, messageKey: null };
  try {
    const resolution = await resolveEntry(sessionId, entry.path);
    const text = resolution.link_target ?? resolution.target_path;
    return {
      text,
      messageKey: text === null
        ? resolutionErrorKey(resolution.resolution_error_code)
        : null,
    };
  } catch (error) {
    return { text: null, messageKey: resolutionErrorKey(sftpErrorCode(error)) };
  }
}

/** 一次下载操作的类型分类结果；`notices` 按提示 key 聚合，避免重复弹窗。 */
export interface DownloadClassification {
  directories: SftpEntry[];
  files: SftpEntry[];
  notices: { messageKey: string; entries: SftpEntry[] }[];
}

/**
 * 按“下载一次”的语义对选中的条目分类。
 *
 * 真实目录走目录流程；普通文件走文件流程；符号链接先解析一次，目标为目录 / 文件
 * 时进入对应流程，断链或目标为特殊文件时不下载并提示。返回的分类结果只是纯数据，
 * 下载流程与提示由调用方执行。
 */
export async function classifyDownloadTargets(
  sessionId: string,
  entries: SftpEntry[],
): Promise<DownloadClassification> {
  const directories: SftpEntry[] = [];
  const files: SftpEntry[] = [];
  const notices = new Map<string, SftpEntry[]>();
  const addNotice = (messageKey: string, entry: SftpEntry) => {
    const group = notices.get(messageKey);
    if (group) group.push(entry);
    else notices.set(messageKey, [entry]);
  };

  for (const entry of entries) {
    if (isDirectory(entry)) {
      directories.push(entry);
      continue;
    }
    if (isRegularFile(entry)) {
      files.push(entry);
      continue;
    }
    if (isSpecial(entry)) {
      addNotice("fileManager.specialFileCannotDownload", entry);
      continue;
    }

    try {
      const resolution = await resolveEntry(sessionId, entry.path);
      const outcome = resolutionOutcome(resolution);
      if (outcome.type === "directory") {
        directories.push(entry);
      } else if (outcome.type === "file") {
        files.push(entry);
      } else {
        addNotice(outcome.messageKey, entry);
      }
    } catch (error) {
      addNotice(resolutionErrorKey(sftpErrorCode(error)), entry);
    }
  }

  return {
    directories,
    files,
    notices: Array.from(notices, ([messageKey, groupedEntries]) => ({
      messageKey,
      entries: groupedEntries,
    })),
  };
}
