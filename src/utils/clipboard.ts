/**
 * 共享剪贴板工具函数
 *
 * 集中管理 navigator.clipboard 访问，统一错误处理，
 * 避免各组件中重复 try/catch 模式。
 */

/**
 * 将文本复制到系统剪贴板。
 * 失败时静默忽略（如剪贴板权限未授予）。
 */
export async function copyToClipboard(text: string): Promise<void> {
  try {
    await navigator.clipboard.writeText(text);
  } catch {
    // 剪贴板写入失败 — 静默忽略
  }
}

/**
 * 从系统剪贴板读取文本。
 * 失败或为空时返回空字符串。
 */
export async function readFromClipboard(): Promise<string> {
  try {
    const text = await navigator.clipboard.readText();
    return text ?? "";
  } catch {
    return "";
  }
}
