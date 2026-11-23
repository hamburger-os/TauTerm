import type { TerminalSurfaceSource } from "../../components/Terminal/TerminalSurface";
import { base64ToBytes, type RttChunk } from "./model.ts";

/**
 * 数据源适配器：把 RTT Channel 的 canonical chunk 流接到共享终端 surface。
 *
 * 每个 RTT Session 只创建一个实例；surface 在 generation / Channel 变化时通过
 * `key` 重挂载，并借 `attach` 重新回放当前 Channel 的可见历史。序列去重与回放
 * 属于数据源，xterm 只消费已经排好序的字节：`append` 在 `attach` 之前到达的批次
 * 会被丢弃，由 `attach` 时的完整回放补齐，因此切换 Terminal/Log/HEX 视图不会
 * 丢行，也不会重复写。
 *
 * `clearReplayHistory` 记录一个按 `generation:channel` 的回放水位：宿主清屏后，
 * surface 重挂载只回放水位之后的 chunk。原始数据（Log/HEX、Rust host history、
 * Session Data Log）不被修改——"清屏"是终端呈现层的动作，不是删除采集数据。
 *
 * generation 的唯一来源是 runtime snapshot（`getGeneration`），不能从 `getChunks()`
 * 推断：surface 可能先挂载、历史随后才经 IPC 到达（首次连接），断线重连也会让
 * 新 generation 先以空 buffer 挂载。若用"第一次带数据的 attach"当 generation，
 * 清屏水位会被下一次挂载误判为旧 generation 而清掉，刚清空的内容又被回放。
 */
export interface RttTerminalSource extends TerminalSurfaceSource {
  append(chunks: readonly RttChunk[]): void;
}

export interface RttTerminalSourceOptions {
  /** 当前 Channel 的有界历史（按 sequence 升序） */
  getChunks(): readonly RttChunk[];
  /** 把键盘/粘贴产生的字节写到当前 Down Channel */
  send(data: Uint8Array): void;
  /** 当前选中的 Channel；清屏水位按 generation:channel 记录。 */
  getChannelIndex(): number | null;
  /** runtime snapshot 的 generation；同一 generation 内重挂载必须保留清屏水位。 */
  getGeneration(): number | null;
}

function replayKey(generation: number, channel: number): string {
  return `${generation}:${channel}`;
}

export function createRttTerminalSource(options: RttTerminalSourceOptions): RttTerminalSource {
  let write: ((data: Uint8Array | string) => void) | null = null;
  let lastSequence = -1;
  let generation: number | null = null;
  /** generation:channel → 该视图已清屏到的最后一个 sequence */
  const clearedThrough = new Map<string, number>();

  const syncGeneration = (): number | null => {
    const next = options.getGeneration();
    if (next !== null && next !== generation) {
      // A new generation is a new console: reconnect/restart replays fully.
      generation = next;
      clearedThrough.clear();
    }
    return generation;
  };

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
      syncGeneration();
      const chunks = options.getChunks();
      for (const chunk of chunks) {
        const cleared = clearedThrough.get(replayKey(chunk.generation, chunk.channel_index));
        if (cleared !== undefined && chunk.sequence <= cleared) continue;
        writeChunk(chunk);
      }
      // Keep append's dedupe aligned even when every chunk sat below the watermark.
      for (const chunk of chunks) {
        if (chunk.sequence > lastSequence) lastSequence = chunk.sequence;
      }
      return () => {
        write = null;
      };
    },
    append: (chunks) => {
      // Batches that arrive before the surface opened are covered by attach's replay.
      if (!write) return;
      for (const chunk of chunks) writeChunk(chunk);
    },
    clearReplayHistory: () => {
      const channel = options.getChannelIndex();
      const currentGeneration = syncGeneration();
      if (channel == null || currentGeneration == null) return;
      const chunks = options.getChunks();
      let through = -1;
      for (const chunk of chunks) {
        if (chunk.channel_index !== channel) continue;
        if (chunk.sequence > through) through = chunk.sequence;
      }
      // Nothing has been received yet: there is no replay window to drop.
      if (through >= 0) clearedThrough.set(replayKey(currentGeneration, channel), through);
    },
  };
}
