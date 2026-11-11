/**
 * 共享剪贴板工具函数
 *
 * 终端剪贴板统一走 Tauri 的原生 clipboard 插件（capability 只授予读写文本），
 * 不使用 `navigator.clipboard`：Tauri 窗口不会响应 WebView2 的
 * `CLIPBOARD_READ` 权限询问，`navigator.clipboard.readText()` 在 Windows 上会
 * 静默失败，导致“粘贴没反应”。读取结果必须可判别，调用方才能把失败告诉用户，
 * 而不是把它当成“剪贴板为空”。
 */
import { readText, writeText } from "@tauri-apps/plugin-clipboard-manager";

export type ClipboardReadResult =
  | { kind: "ok"; text: string }
  | { kind: "empty" }
  | { kind: "unavailable"; error: string };

/**
 * 将文本写入系统剪贴板。
 * 返回是否写入成功；调用方负责决定是否提示失败。
 */
export async function copyToClipboard(text: string): Promise<boolean> {
  try {
    await writeText(text);
    return true;
  } catch {
    return false;
  }
}

/**
 * 读取系统剪贴板文本，并把“空”和“不可用”区分开。
 */
export async function readClipboardText(): Promise<ClipboardReadResult> {
  try {
    const text = await readText();
    return text ? { kind: "ok", text } : { kind: "empty" };
  } catch (error) {
    return {
      kind: "unavailable",
      error: error instanceof Error ? error.message : String(error),
    };
  }
}
