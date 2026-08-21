/**
 * 流式数据展示工具（网络调试会话 / 终端 Dual 模式共用）
 *
 * 从 TerminalView 抽取的纯函数：字节拼接、DualLine 格式化、文本规范化，
 * 以及流式分帧器 StreamFramer（按分隔符 + 超时将数据流切分为帧）。
 * 网络调试会话的对端数据流（TCP 流式 / UDP 按数据报）复用同一套展示语义。
 */

import type { DualLine } from "../components/Terminal/DualPane";

/** 拼接多个 Uint8Array */
export function concatBytes(chunks: Uint8Array[]): Uint8Array {
  const totalLen = chunks.reduce((s, c) => s + c.length, 0);
  const out = new Uint8Array(totalLen);
  let off = 0;
  for (const c of chunks) { out.set(c, off); off += c.length; }
  return out;
}

/** 将原始字节数据转换为 DualLine 基础结构（id 由调用方分配以保证单调递增） */
export function dataToDualLine(data: Uint8Array, direction: "RX" | "TX"): Omit<DualLine, "id"> {
  const now = new Date();
  const hh = String(now.getHours()).padStart(2, "0");
  const mm = String(now.getMinutes()).padStart(2, "0");
  const ss = String(now.getSeconds()).padStart(2, "0");
  const ms = String(now.getMilliseconds()).padStart(3, "0");
  const timestamp = `${hh}:${mm}:${ss}.${ms}`;

  // 剥离尾部帧分隔符 \r \n（行结构已体现换行，不在面板中重复显示）
  let cleanLen = data.length;
  while (cleanLen > 0) {
    const b = data[cleanLen - 1];
    if (b === 0x0d || b === 0x0a) cleanLen--;
    else break;
  }
  const cleanData = cleanLen === data.length ? data : data.slice(0, cleanLen);

  // ASCII text：帧中间的控制字符渲染为 Unicode 控制图片符号
  // \r → ␍ (U+240D)、\n → ␊ (U+240A)，其余不可打印字符显示为 .
  const textParts: string[] = [];
  for (let i = 0; i < cleanData.length; i++) {
    const b = cleanData[i];
    if (b === 0x0d) { textParts.push("␍"); continue; }
    if (b === 0x0a) { textParts.push("␊"); continue; }
    if (b >= 32 && b <= 126) {
      textParts.push(String.fromCharCode(b));
    } else {
      textParts.push(".");
    }
  }

  // HEX 字符串（大写，字节间空格，每 8 字节额外空格分组）
  const hexParts: string[] = [];
  for (let i = 0; i < cleanData.length; i++) {
    if (i > 0) hexParts.push(" ");
    if (i > 0 && i % 8 === 0) hexParts.push(" "); // 第 8/9 字节之间额外空格
    hexParts.push(cleanData[i].toString(16).padStart(2, "0"));
  }

  return { timestamp, direction, text: textParts.join(""), hex: hexParts.join("") };
}

/**
 * 规范化按会话编码解码后的帧文本（Dual 模式文本栏）。
 *
 * 与字节版 dataToDualLine 的展示语义对齐：
 * - 剥离尾部帧分隔符 \r\n（hex 栏 cleanData 已剥离，避免每帧多出 ␍␊）
 * - 控制字符映射：\r → ␍、\n → ␊、其余 <0x20 → .
 */
export function normalizeDecodedText(text: string): string {
  let end = text.length;
  while (end > 0) {
    const c = text.charCodeAt(end - 1);
    if (c !== 0x0d && c !== 0x0a) break;
    end--;
  }
  let out = "";
  for (let i = 0; i < end; i++) {
    const c = text.charCodeAt(i);
    if (c === 0x0d) out += "␍";
    else if (c === 0x0a) out += "␊";
    else if (c < 0x20) out += ".";
    else out += text[i];
  }
  return out;
}

/** 分帧器内部状态 */
interface FrameState {
  buffer: Uint8Array[];
  timer: ReturnType<typeof setTimeout>;
  generation: number;
}

/**
 * 流式分帧器：按分隔符 + 超时将数据流切分为帧（单 key 版本）。
 *
 * 语义与 TerminalView 的 Dual 模式分帧一致：
 * - 字节级扫描 `\r\n` / `\n` / `\r`（优先级 `\r\n` > `\n` > `\r`）；
 * - 无分隔符的数据缓冲等待超时（frameTimeoutMs）后整体成帧；
 * - 代次计数防止过期定时器误触发。
 *
 * 网络调试会话的 TCP 流式对端用它把字节流切成可读帧（类似串口 Dual 分帧）；
 * UDP 对端天然按数据报成帧，无需使用。
 */
export class StreamFramer {
  private frame?: FrameState;

  constructor(private frameTimeoutMs: number) {}

  push(data: Uint8Array, onFrame: (frame: Uint8Array) => void): void {
    if (this.frame?.timer) clearTimeout(this.frame.timer);
    const generation = (this.frame?.generation ?? 0) + 1;

    const chunks = this.frame?.buffer ? [...this.frame.buffer, data] : [data];
    let remaining = concatBytes(chunks);

    // 字节级扫描分隔符
    while (remaining.length > 0) {
      let delimIdx = -1;
      let delimLen = 0;
      for (let i = 0; i < remaining.length; i++) {
        if (remaining[i] === 0x0d && i + 1 < remaining.length && remaining[i + 1] === 0x0a) {
          delimIdx = i; delimLen = 2; break;
        }
        if (remaining[i] === 0x0a) {
          delimIdx = i; delimLen = 1; break;
        }
        if (remaining[i] === 0x0d) {
          delimIdx = i; delimLen = 1; break;
        }
      }
      if (delimIdx < 0) break;
      const lineEnd = delimIdx + delimLen;
      onFrame(remaining.slice(0, lineEnd));
      remaining = remaining.slice(lineEnd);
    }

    // 剩余无分隔符的数据 → 缓冲并等待超时
    if (remaining.length > 0) {
      const capturedGen = generation;
      const timer = setTimeout(() => {
        if (this.frame && this.frame.generation === capturedGen && this.frame.buffer.length > 0) {
          const all = concatBytes(this.frame.buffer);
          if (all.length > 0) onFrame(all);
        }
        if (this.frame && this.frame.generation === capturedGen) {
          delete this.frame;
        }
      }, this.frameTimeoutMs);
      this.frame = { buffer: [remaining], timer, generation };
    } else {
      delete this.frame;
    }
  }

  /** 清理未决的定时器（组件卸载时调用） */
  dispose(): void {
    if (this.frame?.timer) clearTimeout(this.frame.timer);
    delete this.frame;
  }
}
