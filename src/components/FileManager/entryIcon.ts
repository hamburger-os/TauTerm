/**
 * 文件条目图标映射
 *
 * 按 is_dir + 扩展名返回 emoji 图标与分类。
 * 列表视图（FileRow）与网格视图（FileGrid）共用，保证图标永不漂移。
 */
import type { SftpEntry } from "./types";

export type EntryCategory =
  | "folder"
  | "code"
  | "text"
  | "archive"
  | "image"
  | "media"
  | "binary"
  | "generic";

/** 扩展名 → 分类（仅文件；目录单独判定） */
const EXT_CATEGORY: Record<string, EntryCategory> = {
  // 代码
  ".c": "code", ".cpp": "code", ".h": "code", ".hpp": "code", ".rs": "code",
  ".go": "code", ".java": "code", ".js": "code", ".ts": "code", ".jsx": "code",
  ".tsx": "code", ".py": "code", ".rb": "code", ".lua": "code", ".sh": "code",
  ".bash": "code", ".zsh": "code",
  // 文本
  ".txt": "text", ".log": "text", ".md": "text", ".json": "text", ".xml": "text",
  ".yaml": "text", ".yml": "text", ".toml": "text", ".conf": "text", ".ini": "text",
  ".cfg": "text", ".env": "text", ".gitignore": "text", ".editorconfig": "text",
  // 压缩包
  ".zip": "archive", ".tar": "archive", ".gz": "archive", ".tgz": "archive",
  ".bz2": "archive", ".xz": "archive", ".7z": "archive", ".rar": "archive",
  // 图片
  ".png": "image", ".jpg": "image", ".jpeg": "image", ".gif": "image",
  ".svg": "image", ".bmp": "image", ".webp": "image", ".ico": "image",
  // 音视频
  ".mp3": "media", ".wav": "media", ".flac": "media", ".mp4": "media",
  ".avi": "media", ".mkv": "media", ".mov": "media",
  // 可执行 / 二进制
  ".exe": "binary", ".bin": "binary", ".so": "binary", ".dll": "binary",
  ".a": "binary", ".o": "binary", ".out": "binary", ".elf": "binary",
};

const CATEGORY_EMOJI: Record<EntryCategory, string> = {
  folder: "\u{1F4C1}",      // 📁
  code: "\u{1F4DC}",        // 📜
  text: "\u{1F4C4}",        // 📄
  archive: "\u{1F4E6}",     // 📦
  image: "\u{1F5BC}\uFE0F", // 🖼️
  media: "\u{1F3B5}",       // 🎵
  binary: "\u2699\uFE0F",   // ⚙️
  generic: "\u2753",        // ❓
};

/** 分类对应的 i18n 文案 key */
export const CATEGORY_LABEL_KEYS: Record<EntryCategory, string> = {
  folder: "fileManager.catFolder",
  code: "fileManager.catCode",
  text: "fileManager.catText",
  archive: "fileManager.catArchive",
  image: "fileManager.catImage",
  media: "fileManager.catMedia",
  binary: "fileManager.catBinary",
  generic: "fileManager.catGeneric",
};

export function getEntryCategory(entry: SftpEntry): EntryCategory {
  if (entry.is_dir) return "folder";
  const dot = entry.name.lastIndexOf(".");
  if (dot === -1) return "generic";
  return EXT_CATEGORY[entry.name.slice(dot).toLowerCase()] ?? "generic";
}

export function getEntryIcon(entry: SftpEntry): string {
  return CATEGORY_EMOJI[getEntryCategory(entry)];
}

/** 文件夹图标的唯一真源（父目录入口与普通文件夹共用，避免图标漂移） */
export function getFolderIcon(): string {
  return CATEGORY_EMOJI.folder;
}
