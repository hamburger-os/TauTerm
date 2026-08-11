/**
 * 字符编码清单（会话级，连接时选定，连接后不可变）
 *
 * id 同时作为 TextDecoder label 与 Rust 侧 encoding_rs 标签，
 * 两套引擎对以下值均原生支持（WHATWG 标准编码表）。
 * label 为专有名词，无需 i18n。
 */
export interface CharsetOption {
  id: string;
  label: string;
}

export const CHARSETS: CharsetOption[] = [
  { id: "utf-8", label: "UTF-8" },
  { id: "gbk", label: "GBK" },
  { id: "gb18030", label: "GB18030" },
  { id: "big5", label: "Big5" },
  { id: "shift-jis", label: "Shift-JIS" },
  { id: "euc-jp", label: "EUC-JP" },
  { id: "euc-kr", label: "EUC-KR" },
  { id: "iso-8859-1", label: "ISO-8859-1" },
];

export const DEFAULT_ENCODING = "utf-8";

/** 按 id 查找编码标签；未知 id 如实显示原始 id，不伪装成默认编码 */
export function charsetLabel(id: string): string {
  return CHARSETS.find(c => c.id === id)?.label ?? id;
}
