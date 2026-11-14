/**
 * 文件条目类型判定
 *
 * 目录项类型的唯一判定入口。`entry_type` 是后端给出的唯一真源，列表/网格/菜单/
 * 排序/下载/删除/属性等所有“是不是目录、文件、链接、特殊文件”的决策都必须经过
 * 本模块，禁止在其它文件里直接比较 `entry_type` 或派生布尔字段。
 */
import type { SftpEntry, SftpEntryType } from "./types";

/** 判定所需的最小输入：目录项本身或只带类型的等价对象。 */
export type EntryKindSource = Pick<SftpEntry, "entry_type">;

/** 读取目录项类型（后端 `entry_type` 为唯一真源）。 */
export function entryKindOf(entry: EntryKindSource): SftpEntryType {
  return entry.entry_type;
}

/** 真实目录；符号链接即使指向目录也不算目录。 */
export function isDirectory(entry: EntryKindSource): boolean {
  return entryKindOf(entry) === "directory";
}

/** 普通文件；符号链接即使指向文件也不算普通文件。 */
export function isRegularFile(entry: EntryKindSource): boolean {
  return entryKindOf(entry) === "file";
}

/** 符号链接（lstat 语义，是否跟随由激活/下载流程单独决定）。 */
export function isSymlink(entry: EntryKindSource): boolean {
  return entryKindOf(entry) === "symlink";
}

/** 特殊文件：FIFO、socket、块设备、字符设备及其它非常规类型。 */
export function isSpecial(entry: EntryKindSource): boolean {
  const kind = entryKindOf(entry);
  return kind !== "directory" && kind !== "file" && kind !== "symlink";
}

/** 能否直接进入 Remote Document 工作流；符号链接需先解析一次再决定。 */
export function canOpenAsDocument(entry: EntryKindSource): boolean {
  return isRegularFile(entry);
}
