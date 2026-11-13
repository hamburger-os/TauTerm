import type { TerminalSurfaceSource } from "../../components/Terminal/TerminalSurface";
import { base64ToBytes, type RttChunk } from "./model";

/**
 * 数据源适配器：把 RTT Channel 的 canonical chunk 流接到共享终端 surface。
 *
 * 每个 RTT Session 只创建一个实例；surface 在 generation / Channel 变化时通过
 * `key` 重挂载，并借 `attach` 重新回放当前 Channel 的可见历史。序列去重与回放
 * 属于数据源，xterm 只消费已经排好序的字节：`append` 在 `attach` 之前到达的批次
 * 会被丢弃，由 `attach` 时的完整回放补齐，因此切换 Terminal/Log/HEX 视图不会
 * 丢行，也不会重复写。
 */
export interface RttTerminalSource extends TerminalSurfaceSource {
  append(chunks: readonly RttChunk[]): void;
}

export interface RttTerminalSourceOptions {
  /** 当前 Channel 的有界历史（按 sequence 升序） */
  getChunks(): readonly RttChunk[];
  /** 把键盘/粘贴产生的字节写到当前 Down Channel */
  send(data: Uint8Array): void;
}

export function createRttTerminalSource(options: RttTerminalSourceOptions): RttTerminalSource {
  let write: ((data: Uint8Array | string) => void) | null = null;
  let lastSequence = -1;

  const writeChunk = (chunk: RttChunk) => {
    if (chunk.sequence <= lastSequence) return;
    lastSequence = chunk.sequence;
    write?.(base64ToBytes(chunk.data_b64));
  };

  return {
    input: (data) => options.send(new TextEncoder().encode(data)),
    attach: (nextWrite) => {
      write = nextWrite;
      lastSequence = -1;
      // Replay the bounded history so a remounted view restores the console instead
      // of starting blank.
      for (const chunk of options.getChunks()) writeChunk(chunk);
      return () => {
        write = null;
      };
    },
    append: (chunks) => {
      // Batches that arrive before the surface opened are covered by attach's replay.
      if (!write) return;
      for (const chunk of chunks) writeChunk(chunk);
    },
  };
}
