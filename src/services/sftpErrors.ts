/**
 * SFTP 错误契约（与 Rust `SftpError` 对齐）。
 *
 * 后端所有 SFTP 命令都以 `{ code, message }` 结构化错误拒绝，`message` 是后端
 * 生成的、面向用户的说明；`code` 只用于区分重试/失效/不可用等语义。展示一律
 * 走 `sftpErrorMessage`，避免把错误对象字符串化成 "[object Object]"。
 */
export type SftpErrorCode =
  | "not_found"
  | "permission_denied"
  | "conflict"
  | "closed"
  | "timeout"
  | "unsupported"
  | "loop"
  | "io"
  | "protocol";

export interface SftpError {
  code: SftpErrorCode;
  message: string;
}

function asRecord(value: unknown): Record<string, unknown> | null {
  return typeof value === "object" && value !== null
    ? (value as Record<string, unknown>)
    : null;
}

/** 判断一个命令拒绝值是否是后端 typed SFTP 错误。 */
export function isSftpError(value: unknown): value is SftpError {
  const record = asRecord(value);
  return (
    record !== null
    && typeof record.code === "string"
    && typeof record.message === "string"
  );
}

/** typed SFTP 错误的 code；非 typed 错误（旧式字符串/普通 Error）返回 null。 */
export function sftpErrorCode(value: unknown): SftpErrorCode | null {
  return isSftpError(value) ? (value.code as SftpErrorCode) : null;
}

/**
 * 把任意命令拒绝值转换为用户可见文本。
 *
 * typed SFTP 错误直接使用后端 `message`（已是精确的用户说明）；其它值保持既有
 * 行为（`String(value)`，覆盖字符串错误与普通 `Error`）。
 */
export function sftpErrorMessage(value: unknown): string {
  const record = asRecord(value);
  if (record !== null && typeof record.message === "string") {
    return record.message;
  }
  return String(value);
}
