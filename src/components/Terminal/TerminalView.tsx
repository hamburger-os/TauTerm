import { useEffect, useRef, useCallback, useState, useMemo } from "react";
import { useTranslation } from "react-i18next";
import { AnimatePresence, motion } from "framer-motion";
import { useSession } from "../../context/SessionContext";
import { useTheme } from "../../context/ThemeContext";
import { useKeyboard } from "../../hooks/useKeyboard";
import { ACTION_IDS } from "../../shortcuts/actionIds";
import Icon from "../common/Icon";
import TerminalInstance from "./Terminal";
import DualPane from "./DualPane";
import type { DualLine } from "./DualPane";
import SearchBar from "./SearchBar";
import styles from "./Terminal.module.css";

const BYTES_PER_LINE = 16;
const HEX_HALF_WIDTH = 8 * 3 - 1; // 8 字节 hex 列最大宽度 (8×2 位 + 7 个空格 = 23)

/** Dual 模式帧超时默认值：50ms 内未收到新数据则视为一帧结束，可在连接配置中覆盖 */
const DUAL_FRAME_TIMEOUT_DEFAULT_MS = 50;

/** 拼接多个 Uint8Array */
function concatBytes(chunks: Uint8Array[]): Uint8Array {
  const totalLen = chunks.reduce((s, c) => s + c.length, 0);
  const out = new Uint8Array(totalLen);
  let off = 0;
  for (const c of chunks) { out.set(c, off); off += c.length; }
  return out;
}

/**
 * 格式化单行 Hex Dump（最多 16 字节）
 *
 * 固定宽度 78 字符，保证多次输出 `\r` 覆盖对齐。
 * 格式：8位偏移量 + 2空格 + 左8字节hex + 2空格 + 右8字节hex + 2空格 + |ASCII|
 */
function formatHexLine(data: Uint8Array, offset: number): string {
  const chunk = data.slice(0, BYTES_PER_LINE);
  const offsetStr = offset.toString(16).padStart(8, "0");

  // Hex 列：前 8 字节
  const leftHexParts: string[] = [];
  for (let j = 0; j < 8 && j < chunk.length; j++) {
    leftHexParts.push(chunk[j].toString(16).padStart(2, "0"));
  }
  const leftHex = leftHexParts.join(" ").padEnd(HEX_HALF_WIDTH, " ");

  // Hex 列：后 8 字节
  const rightHexParts: string[] = [];
  for (let j = 8; j < BYTES_PER_LINE && j < chunk.length; j++) {
    rightHexParts.push(chunk[j].toString(16).padStart(2, "0"));
  }
  const rightHex = rightHexParts.join(" ").padEnd(HEX_HALF_WIDTH, " ");

  // 第 8/9 字节之间额外空格
  const hex = `${leftHex}  ${rightHex}`;

  // ASCII 列：固定 16 字符宽，不足补空格
  const asciiParts: string[] = [];
  for (let j = 0; j < BYTES_PER_LINE; j++) {
    if (j < chunk.length) {
      const b = chunk[j];
      asciiParts.push((b >= 32 && b <= 126) ? String.fromCharCode(b) : ".");
    } else {
      asciiParts.push(" ");
    }
  }
  const ascii = asciiParts.join("");

  return `${offsetStr}  ${hex}  |${ascii}|`;
}

/** 将原始字节数据转换为 DualLine 基础结构（id 由调用方分配以保证单调递增） */
function dataToDualLine(data: Uint8Array, direction: "RX" | "TX"): Omit<DualLine, "id"> {
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
function normalizeDecodedText(text: string): string {
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

/**
 * 终端区域管理器
 *
 * 同时渲染所有已连接标签页的终端实例，使用 CSS opacity 控制可见性。
 * 非活跃标签页的终端保持在 DOM 中并继续接收数据，切换时无需重建。
 */
export default function TerminalView() {
  const { t } = useTranslation();
  const { state, sendData, disconnect, onSessionData, onDataSent } = useSession();
  const { fontSize, bufferLines } = useTheme();
  const { registerAction } = useKeyboard();
  const writeRefs = useRef<Map<string, (data: Uint8Array | string) => void>>(new Map());
  const terminalRefs = useRef<Map<string, any>>(new Map());
  const [searchVisible, setSearchVisible] = useState(false);
  /** per-Tab 搜索状态持久化：key 为 tabId，在 SearchBar 关闭时写入，打开时恢复 */
  const searchStateRef = useRef<Map<string, { query: string; caseSensitive: boolean }>>(new Map());
  const currentSearchState = state.activeTabId ? (searchStateRef.current.get(state.activeTabId) ?? { query: "", caseSensitive: false }) : { query: "", caseSensitive: false };

  // 关闭搜索栏并归还焦点到终端
  const handleSearchClose = useCallback(() => {
    setSearchVisible(false);
    setTimeout(() => {
      activeTermRef.current?.terminal?.focus();
    }, 0);
  }, []);

  // 搜索栏关闭时保存当前搜索状态到 per-Tab Map
  const handleSearchSaveState = useCallback((query: string, caseSensitive: boolean) => {
    if (state.activeTabId) {
      searchStateRef.current.set(state.activeTabId, { query, caseSensitive });
    }
  }, [state.activeTabId]);
  const activeTermRef = useRef<any>(null);
  const hexOffsetsRef = useRef<Map<string, number>>(new Map());
  /** HEX 模式待补行缓冲区：存不够 16 字节的剩余字节，凑满 16 后输出完整行并换行 */
  const hexPendingRef = useRef<Map<string, { offset: number; data: Uint8Array }>>(new Map());
  /** 分帧缓冲区（Dual 模式专用）：按超时/分隔符分帧，generation 防竞态 */
  const frameBufRef = useRef<Map<string, { buffer: Uint8Array[]; timer: ReturnType<typeof setTimeout> | null; generation: number }>>(new Map());
  /** Dual 模式行数据：使用 useState 触发 React 正常重渲染（避免 key hack 导致 remount） */
  const [dualLines, setDualLines] = useState<Map<string, DualLine[]>>(new Map());
  /** RAF 批量提交缓冲区：累积同一帧内的行数据，减少 setState 次数 */
  const pendingDualRef = useRef<Map<string, DualLine[]>>(new Map());
  /** text 模式数据批量提交缓冲区：累积同一帧内多包数据（已按会话编码解码为文本），合并为一次 xterm.write */
  const pendingTextRef = useRef<Map<string, string[]>>(new Map());
  /** text 模式 RAF ID：用于批量提交的去重 */
  const textRafIdRef = useRef<number | null>(null);
  /**
   * 每会话流式解码器：按包 decode({stream:true})，自动处理跨包被切断的多字节字符。
   * label 用于校验会话编码是否变更（编辑编码后重连时缓存失效，避免旧解码器产生乱码）。
   * 生命周期与连接绑定（断连/删除时在 cleanup effect 中终结）。
   */
  const decodersRef = useRef<Map<string, { label: string; decoder: TextDecoder }>>(new Map());
  /** RAF ID：用于批量提交的去重 */
  const rafIdRef = useRef<number | null>(null);
  /** 每个 session 的单调递增行号计数器：用于生成稳定的 React key */
  const lineIdCounterRef = useRef<Map<string, number>>(new Map());
  /** Ref 镜像 dualLines keys：用于在 setDualLines 更新器外部同步检查已断连会话 ID */
  const dualLinesKeysRef = useRef<Set<string>>(new Set());
  /** Ref 镜像 context bufferLines：避免 flushDualLines 闭包陈旧 */
  const bufferLinesRef = useRef(bufferLines);
  bufferLinesRef.current = bufferLines;
  const tabsRef = useRef(state.tabs);
  const viewportRef = useRef<HTMLDivElement>(null);
  tabsRef.current = state.tabs;

  // 所有已连接的标签页（需要保持终端实例存活）
  const connectedTabs = state.tabs.filter(
    t => t.state === "connected" || t.state === "transferring"
  );
  const activeTab = state.tabs.find(t => t.id === state.activeTabId);

  /** 已连接标签页 ID 的稳定字符串：避免 cleanup effect 因数组引用变化而过度触发 */
  const connectedTabIds = useMemo(
    () => state.tabs
      .filter(t => t.state === "connected" || t.state === "transferring")
      .map(t => t.id)
      .sort()
      .join(","),
    [state.tabs]
  );

  // 同步 activeTermRef 指向当前活跃标签页的终端引用
  useEffect(() => {
    activeTermRef.current = state.activeTabId
      ? terminalRefs.current.get(state.activeTabId) ?? null
      : null;
  }, [state.activeTabId]);

  // 注册 Ctrl+F 搜索快捷键
  useEffect(() => {
    registerAction(ACTION_IDS.TERMINAL_SEARCH, () => setSearchVisible(v => !v));
  }, [registerAction]);

  // 注册 Ctrl+Shift+A 全选快捷键
  useEffect(() => {
    registerAction(ACTION_IDS.TERMINAL_SELECT_ALL, () => {
      activeTermRef.current?.terminal?.selectAll();
    });
  }, [registerAction]);

  // Tab 切换时仅关闭 SearchBar overlay，不重置 per-Tab 搜索状态（恢复由 SearchBar 通过 savedQuery 完成）
  useEffect(() => {
    setSearchVisible(false);
  }, [state.activeTabId]);

  // 取消待处理的 RAF，避免组件卸载后触发 setState
  useEffect(() => {
    return () => {
      if (rafIdRef.current !== null) {
        cancelAnimationFrame(rafIdRef.current);
        rafIdRef.current = null;
      }
      if (textRafIdRef.current !== null) {
        cancelAnimationFrame(textRafIdRef.current);
        textRafIdRef.current = null;
      }
    };
  }, []);

  // 保持 dualLinesKeysRef 与 dualLines state 同步，供清理 effect 在 updater 外部使用
  useEffect(() => {
    dualLinesKeysRef.current = new Set(dualLines.keys());
  }, [dualLines]);

  // ── 共享分帧处理：按分隔符 + 超时将数据流切分为帧，回调 onFrame ──
  // 使用字节级扫描，不经过 TextDecoder/TextEncoder 往返，避免二进制数据损坏。
  const processIncomingFrame = useCallback((
    sessionId: string,
    data: Uint8Array,
    frameTimeoutMs: number,
    onFrame: (frameData: Uint8Array) => void,
  ) => {
    const frame = frameBufRef.current.get(sessionId);

    // 取消已有的超时定时器
    if (frame?.timer) clearTimeout(frame.timer);

    // 代次计数器：每次追加递增，超时回调中比对防止过期定时器误触发
    const generation = (frame?.generation ?? 0) + 1;

    // 拼接缓冲区 + 新数据
    const chunks = frame?.buffer ? [...frame.buffer, data] : [data];
    let remaining = concatBytes(chunks);

    // 字节级扫描分隔符：\r\n (0x0D 0x0A)、\n (0x0A)、\r (0x0D)
    // 优先级：\r\n > \n > \r，避免将 \r\n 误拆为两个分隔符
    while (remaining.length > 0) {
      let delimIdx = -1;
      let delimLen = 0;

      for (let i = 0; i < remaining.length; i++) {
        // \r\n 双字节分隔符优先
        if (remaining[i] === 0x0d && i + 1 < remaining.length && remaining[i + 1] === 0x0a) {
          delimIdx = i; delimLen = 2; break;
        }
        // \n 单字节分隔符
        if (remaining[i] === 0x0a) {
          delimIdx = i; delimLen = 1; break;
        }
        // \r 单字节分隔符
        if (remaining[i] === 0x0d) {
          delimIdx = i; delimLen = 1; break;
        }
      }

      if (delimIdx < 0) break; // 无分隔符，退出循环

      const lineEnd = delimIdx + delimLen;
      // 直接 slice Uint8Array，保持原始字节不变
      const lineData = remaining.slice(0, lineEnd);
      remaining = remaining.slice(lineEnd);

      onFrame(lineData);
    }

    // 剩余无分隔符的数据 → 缓冲并等待超时
    if (remaining.length > 0) {
      const capturedGen = generation;
      const timer = setTimeout(() => {
        const f = frameBufRef.current.get(sessionId);
        // 代次比对：如果期间有新数据到达（generation 已递增），跳过过期回调
        if (f && f.generation === capturedGen && f.buffer.length > 0) {
          const all = concatBytes(f.buffer);
          if (all.length > 0) onFrame(all);
        }
        // 仅当代次匹配时才清理（不匹配说明已有新 buffer 接管）
        if (f && f.generation === capturedGen) {
          frameBufRef.current.delete(sessionId);
        }
      }, frameTimeoutMs);

      frameBufRef.current.set(sessionId, { buffer: [remaining], timer, generation });
    } else {
      frameBufRef.current.delete(sessionId);
    }
  }, []);

  // ── Dual 模式：将一帧数据转为 DualLine 并推入渲染缓冲区 ──
  // 使用 RAF 批量提交：同一帧内到达的多帧数据合并为一次 setState，减少 GC 压力

  const flushDualLines = useCallback(() => {
    rafIdRef.current = null;
    const pending = pendingDualRef.current;
    if (pending.size === 0) return;

    // 快照当前待提交数据后立即清空原 Map，
    // 避免 React 异步执行 updater 时 pending 已被清空导致数据丢失。
    const snapshot = new Map(pending);
    pending.clear();

    setDualLines(prev => {
      const next = new Map(prev);
      const maxLines = bufferLinesRef.current;
      for (const [sessionId, newLines] of snapshot) {
        const lines = [...(prev.get(sessionId) ?? []), ...newLines];
        if (lines.length > maxLines) {
          lines.splice(0, lines.length - maxLines);
        }
        next.set(sessionId, lines);
      }
      return next;
    });
  }, []);

  // ── text 模式：rAF 批处理合并多包 xterm.write 调用 ──
  // 后端批处理器已合并 16ms 窗口内的数据，但前端可能在一帧内收到多个 emit
  // （不同 session 或同 session 跨窗口）。此处在 rAF 中合并同 session 的多包数据
  // 为单次 xterm.write，减少 ANSI 解析 + 渲染调度次数。
  // 数据包已在事件回调中按会话编码流式解码为文本（处理跨包多字节字符）。
  const flushTextBuffers = useCallback(() => {
    textRafIdRef.current = null;
    const pending = pendingTextRef.current;
    if (pending.size === 0) return;

    // 快照后清空，避免在合并过程中新数据到达导致迭代异常
    const snapshot = new Map(pending);
    pending.clear();

    for (const [sessionId, chunks] of snapshot) {
      const writeFn = writeRefs.current.get(sessionId);
      if (!writeFn) continue;

      // 合并所有解码后的文本 chunk 为单次 write
      const merged = chunks.join("");
      if (merged.length === 0) continue;
      writeFn(merged);
    }
  }, []);

  const pushDualLine = useCallback((
    sessionId: string,
    direction: "RX" | "TX",
    data: Uint8Array,
    decodedText?: string,
  ) => {
    if (data.length === 0) return;
    const base = decodedText !== undefined
      ? { ...dataToDualLine(data, direction), text: normalizeDecodedText(decodedText) }
      : dataToDualLine(data, direction);
    // 跳过空帧（纯分隔符 \r\n 剥离后无内容）
    if (base.text.length === 0 && base.hex.length === 0) return;

    // 单调递增行号：确保 React key 稳定
    const lineId = (lineIdCounterRef.current.get(sessionId) ?? 0) + 1;
    lineIdCounterRef.current.set(sessionId, lineId);
    const newLine: DualLine = { ...base, id: lineId };

    // 推入 RAF 批量缓冲区
    const pending = pendingDualRef.current;
    if (!pending.has(sessionId)) pending.set(sessionId, []);
    pending.get(sessionId)!.push(newLine);

    // 如果当前帧已有待处理的 RAF，复用；否则新建
    if (rafIdRef.current === null) {
      rafIdRef.current = requestAnimationFrame(flushDualLines);
    }
  }, [flushDualLines]);

  // 获取（惰性创建）指定会话的流式解码器，编码取自会话参数（连接时确定）。
  // 缓存条目记录创建时的编码 label：会话编码变更（编辑会话后重连）时，
  // label 不一致则丢弃旧解码器重建，避免旧编码解码器处理新编码字节产生乱码。
  const getSessionDecoder = useCallback((sessionId: string): TextDecoder => {
    const tab = tabsRef.current.find(t => t.id === sessionId);
    const label = typeof tab?.params?.encoding === "string"
      ? tab.params.encoding
      : "utf-8";
    const cached = decodersRef.current.get(sessionId);
    if (cached && cached.label === label) return cached.decoder;
    let decoder: TextDecoder;
    try {
      decoder = new TextDecoder(label);
    } catch {
      // 未知编码标签防御性回退（旧会话参数残留）
      decoder = new TextDecoder("utf-8");
    }
    decodersRef.current.set(sessionId, { label, decoder });
    return decoder;
  }, []);

  // 注册数据回调，将每个 session 的数据路由到对应终端
  useEffect(() => {
    onSessionData((sessionId, data) => {
      const tab = tabsRef.current.find(t => t.id === sessionId);
      if (!tab) return;

      const isDual = tab.params?.data_mode === "dual";
      const writeFn = writeRefs.current.get(sessionId);
      // Dual 模式无需 xterm writeFn，直接推入 DualPane 缓冲区
      if (!isDual && !writeFn) return;

      if (tab.params?.data_mode === "hex") {
        // ── HEX 模式：逐行实时更新 ──
        // 取出上一轮未满 16 字节的待补数据，与新数据拼接
        const pending = hexPendingRef.current.get(sessionId);
        const prev = pending?.data ?? new Uint8Array(0);
        const baseOffset = pending?.offset ?? (hexOffsetsRef.current.get(sessionId) ?? 0);
        const prevHadPending = prev.length > 0;

        const combined = new Uint8Array(prev.length + data.length);
        combined.set(prev);
        combined.set(data, prev.length);

        let pos = 0;
        let curOffset = baseOffset;

        // writeFn 已由前置检查保证非空（非 dual 模式且 writeFn 存在）
        const w = writeFn!;

        // 输出所有已满 16 字节的行
        while (pos + BYTES_PER_LINE <= combined.length) {
          const line = formatHexLine(combined.slice(pos, pos + BYTES_PER_LINE), curOffset);
          if (pos === 0 && prevHadPending) {
            // 替换上一轮显示的待补行：\r 回行首，写入完整行，\r\n 换行
            w("\r" + line + "\r\n");
          } else {
            w(line + "\r\n");
          }
          pos += BYTES_PER_LINE;
          curOffset += BYTES_PER_LINE;
        }

        hexOffsetsRef.current.set(sessionId, curOffset);

        // 不满 16 字节的剩余：先输出（无换行），存起来等下一轮拼接后覆盖
        const remainder = combined.slice(pos);
        if (remainder.length > 0) {
          const line = formatHexLine(remainder, curOffset);
          w("\r" + line); // \r 回到行首准备覆盖，无尾随 \n 所以光标在该行末尾
          hexPendingRef.current.set(sessionId, { offset: curOffset, data: remainder });
        } else {
          hexPendingRef.current.delete(sessionId);
        }
      } else if (tab?.params?.data_mode === "dual") {
        // ── Dual 模式：分帧 → DualPane 双栏渲染 ──
        const timeout = typeof tab?.params?.dual_frame_timeout_ms === "number"
          ? tab.params.dual_frame_timeout_ms
          : DUAL_FRAME_TIMEOUT_DEFAULT_MS;
        processIncomingFrame(sessionId, data, timeout, (frameData) => {
          // 文本栏按会话编码流式解码；hex 栏保持原始字节（dataToDualLine 内部生成）
          const decoded = getSessionDecoder(sessionId).decode(frameData, { stream: true });
          pushDualLine(sessionId, "RX", frameData, decoded);
        });
      } else {
        // ── text 模式：rAF 批处理合并多包数据 ──
        // 后端批处理器已合并 16ms 窗口内的数据，但前端可能在一帧内收到多个 emit。
        // 每个包先按会话编码流式解码（处理跨包被切断的多字节字符），
        // 再推入 pendingTextRef，在 rAF 中合并为单次 xterm.write，
        // 减少 ANSI 解析 + 渲染调度次数。
        const text = getSessionDecoder(sessionId).decode(data, { stream: true });
        const pending = pendingTextRef.current;
        if (!pending.has(sessionId)) pending.set(sessionId, []);
        pending.get(sessionId)!.push(text);
        if (textRafIdRef.current === null) {
          textRafIdRef.current = requestAnimationFrame(flushTextBuffers);
        }
      }
    });
    // 卸载时清除回调，避免未挂载组件的闭包响应数据事件
    return () => { onSessionData(() => {}); };
  }, [onSessionData, pushDualLine, processIncomingFrame, flushTextBuffers]);

  /**
   * 终结会话流式解码器：调用最终 decode() 冲刷内部缓冲的不完整多字节序列
   * （结果为 U+FFFD 标记，断连时无法展示，直接丢弃；仅正确终结生命周期）。
   */
  const flushSessionDecoder = useCallback((sessionId: string) => {
    const entry = decodersRef.current.get(sessionId);
    if (entry) {
      try { entry.decoder.decode(); } catch { /* 终结失败不影响清理流程 */ }
      decodersRef.current.delete(sessionId);
    }
  }, []);

  // 注册发送数据回调（TX）：Dual 模式推入 DualPane；Telnet 本地回显写回终端
  useEffect(() => {
    onDataSent((sessionId, data) => {
      const tab = tabsRef.current.find(t => t.id === sessionId);
      if (!tab) return;
      // ── Dual 模式：TX 帧推入 DualPane 渲染 ──
      if (tab.params?.data_mode === "dual") {
        // data 为后端实际写入设备的字节（文本路径已按会话编码转码）。
        // 一次性解码（TX 帧是完整数据，非字节流）；不复用 RX 流式解码器，
        // 避免双方共享流状态导致跨方向多字节序列污染。
        const label = typeof tab?.params?.encoding === "string"
          ? tab.params.encoding
          : "utf-8";
        let decoded: string;
        try {
          decoded = new TextDecoder(label).decode(data);
        } catch {
          // 未知编码标签防御性回退（旧会话参数残留）
          decoded = new TextDecoder("utf-8").decode(data);
        }
        pushDualLine(sessionId, "TX", data, decoded);
        return;
      }
      // ── Telnet 本地回显：服务器 WONT ECHO 时客户端回显输入 ──
      // 覆盖全部发送路径（键盘 / SendBar 各模式 / Lua 脚本均经 sendData）；
      // 服务器回显时 localEcho=false，天然跳过，无双显。
      // data 为后端实际写入设备的字节（文本路径已按会话编码转码），
      // 需先按会话编码解码回文本再写 xterm，否则非 UTF-8 会话回显乱码。
      // 与上方 Dual 分支一致：用一次性解码器（TX 帧为完整数据），
      // 不复用 RX 流式解码器，避免共享流状态污染。
      if (tab.localEcho) {
        const writeFn = writeRefs.current.get(sessionId);
        if (writeFn) {
          const label = typeof tab?.params?.encoding === "string"
            ? tab.params.encoding
            : "utf-8";
          let echoText: string;
          try {
            echoText = new TextDecoder(label).decode(data);
          } catch {
            echoText = new TextDecoder("utf-8").decode(data);
          }
          writeFn(echoText);
        }
      }
    });
    // 卸载时清除回调，避免未挂载组件的闭包响应 TX 事件
    return () => { onDataSent(() => {}); };
  }, [onDataSent, pushDualLine]);

  // 清理已断开/已删除会话的 writeRefs、terminalRefs 和 Dual 缓冲区
  useEffect(() => {
    const connectedIds = new Set(
      connectedTabIds ? connectedTabIds.split(",").filter(Boolean) : []
    );

    // Phase 1: 从 writeRefs 收集已断连 ID（text/hex 模式会话）
    const toRemove: string[] = [];
    writeRefs.current.forEach((_, id) => {
      if (!connectedIds.has(id)) toRemove.push(id);
    });

    // Phase 2: 从 dualLinesKeysRef 收集已断连的 Dual 模式会话（不在 writeRefs 中）
    const dualDisconnected: string[] = [];
    for (const id of dualLinesKeysRef.current) {
      if (!connectedIds.has(id) && !toRemove.includes(id)) {
        dualDisconnected.push(id);
      }
    }

    // Phase 3: 清理所有 ref（在 setDualLines 外部，保持 updater 纯函数）
    for (const id of toRemove) {
      writeRefs.current.delete(id);
      terminalRefs.current.delete(id);
      hexOffsetsRef.current.delete(id);
      hexPendingRef.current.delete(id);
      const fb = frameBufRef.current.get(id);
      if (fb?.timer) clearTimeout(fb.timer);
      frameBufRef.current.delete(id);
      pendingDualRef.current.delete(id);
      lineIdCounterRef.current.delete(id);
      flushSessionDecoder(id);
    }
    for (const id of dualDisconnected) {
      const fb = frameBufRef.current.get(id);
      if (fb?.timer) clearTimeout(fb.timer);
      frameBufRef.current.delete(id);
      pendingDualRef.current.delete(id);
      lineIdCounterRef.current.delete(id);
      flushSessionDecoder(id);
    }

    // Phase 4: 纯 state 更新（无副作用）
    if (toRemove.length > 0 || dualDisconnected.length > 0) {
      setDualLines(prev => {
        const next = new Map(prev);
        for (const id of toRemove) next.delete(id);
        for (const id of dualDisconnected) next.delete(id);
        return next;
      });
    }
  }, [connectedTabIds, flushSessionDecoder]);

  const handleTermReady = useCallback((sessionId: string, writeFn: (data: Uint8Array | string) => void) => {
    writeRefs.current.set(sessionId, writeFn);
  }, []);

  const handleTermCleanup = useCallback((sessionId: string) => {
    writeRefs.current.delete(sessionId);
    hexOffsetsRef.current.delete(sessionId);
    hexPendingRef.current.delete(sessionId);
    const fb = frameBufRef.current.get(sessionId);
    if (fb?.timer) clearTimeout(fb.timer);
    frameBufRef.current.delete(sessionId);
    pendingDualRef.current.delete(sessionId);
    pendingTextRef.current.delete(sessionId);
    lineIdCounterRef.current.delete(sessionId);
    flushSessionDecoder(sessionId);
    setDualLines(prev => {
      const next = new Map(prev);
      next.delete(sessionId);
      return next;
    });
  }, [flushSessionDecoder]);

  const handleData = useCallback((sessionId: string, data: string) => {
    sendData(sessionId, data);
  }, [sendData]);

  const isActiveTransferring = activeTab?.state === "transferring";

  return (
    <div className={styles.viewport} ref={viewportRef}>
      <div className={`${styles.terminalArea} liquid-glass`}>
        {isActiveTransferring && (
          <motion.div
            className={styles.transferBanner}
            initial={{ opacity: 0, y: -4 }}
            animate={{ opacity: 1, y: 0 }}
          >
            <Icon name="transfer-active" size="sm" className={styles.transferBannerIcon} />
            <span>{t("transfer.transferringBanner", "File transfer in progress...")}</span>
          </motion.div>
        )}

        <div className={styles.terminalsContainer}>
          <AnimatePresence>
            {connectedTabs.map(tab => {
              const isActive = tab.id === state.activeTabId;
              const isDual = tab.params?.data_mode === "dual";

              return (
                <motion.div
                  key={tab.id}
                  className={styles.terminalWrapper}
                  initial={{ opacity: 0 }}
                  animate={{ opacity: isActive ? 1 : 0 }}
                  exit={{ opacity: 0 }}
                  style={{ pointerEvents: isActive ? "auto" : "none" }}
                  transition={{ duration: 0.15 }}
                >
                  {isDual ? (
                    <DualPane
                      key={tab.id}
                      lines={dualLines.get(tab.id) ?? []}
                      fontSize={fontSize}
                      bufferLines={bufferLines}
                    />
                  ) : (
                    <TerminalInstance
                      sessionId={tab.id}
                      onData={(data) => handleData(tab.id, data)}
                      isConnected={tab.state === "connected" || tab.state === "transferring"}
                      isActive={isActive}
                      onTermReady={(writeFn) => handleTermReady(tab.id, writeFn)}
                      onCleanup={handleTermCleanup}
                      fontSize={fontSize}
                      bufferLines={bufferLines}
                      onShowSearch={() => setSearchVisible(true)}
                      onDisconnectSession={() => disconnect(tab.id)}
                      ref={(node) => {
                        if (node) {
                          terminalRefs.current.set(tab.id, node);
                        } else {
                          terminalRefs.current.delete(tab.id);
                        }
                      }}
                    />
                  )}
                </motion.div>
              );
            })}
          </AnimatePresence>

          {connectedTabs.length === 0 && (
            <div className={styles.emptyState}>
              <Icon name="logo" size="2xl" className={styles.emptyIcon} />
              <div>{t("session.noSessions")}</div>
              <div className={styles.emptyHint}>
                {t("session.emptyHint") || "Use Ctrl+Shift+N to create a new session"}
              </div>
            </div>
          )}
        </div>
      </div>

      <SearchBar
        isOpen={searchVisible}
        onClose={handleSearchClose}
        terminal={activeTermRef.current?.terminal ?? null}
        savedQuery={currentSearchState.query}
        savedCaseSensitive={currentSearchState.caseSensitive}
        onSaveState={handleSearchSaveState}
        viewportRef={viewportRef}
      />
    </div>
  );
}
