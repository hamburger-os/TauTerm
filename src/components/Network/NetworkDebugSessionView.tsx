/**
 * 网络调试会话主视图（content_type: "custom" → CustomRenderer → 本组件）
 *
 * 裸视图设计（与串口会话一致）：只承载数据展示区域，无自定义头部/工具条。
 * - 会话身份（名称/端点/角色）由左侧会话树展示，TX/RX 统计由底部状态栏展示；
 * - TCP 对端导航在左侧 SessionSidebar 树：server 容器展开对端子节点，点击路由并选中；
 *   数据模式（dual/text/hex）来自连接参数 `params.data_mode`（会话内不可切换）；
 * - UDP 无连接、无对端：单会话报文网格显示所有来源/目标的时间线，
 *   发送目标由发送栏手动地址（含广播/组播）决定；client 固定远端；
 * - 发送栏由 App.tsx 在全局底部位置渲染，发送目标由发送栏内 TargetBar 选择。
 *
 * 数据流：
 * - TCP 对端经内核 `register_peer_channel` 注册为容器会话的子连接（tabbed=false），
 *   数据事件 `session-data` 以对端 UUID 为 session_id（本组件监听并分帧）；
 * - UDP 由后端单 socket `recv_from` 直接 emit `session-data`（session_id = 容器，
 *   payload 带 `source_addr`），本组件逐报文记网格；
 * - `netdbg-peer-joined/left`、`session-stats` 由 SessionContext 全局监听维护 TCP 对端条目；
 * - TX：TCP 走 `subscribeDataSent`（群发扇出由 SessionContext.sendData 统一处理），
 *   UDP 走 `subscribeNetworkManualSent`。
 */
import { useEffect, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { useTranslation } from "react-i18next";
import { useSession, type NetworkPeerEntry } from "../../context/SessionContext";
import { usePluginSessionStore, type SessionStoreApi } from "../../hooks/usePluginSessionStore";
import { useAutoScroll } from "../../hooks/useAutoScroll";
import DualPane, { type DualLine } from "../Terminal/DualPane";
import ScrollToBottomButton from "../Terminal/ScrollToBottomButton";
import Icon from "../common/Icon";
import { dataToDualLine, normalizeDecodedText, StreamFramer } from "../../utils/streamDisplay";
import UdpPacketGrid from "./UdpPacketGrid";
import styles from "./NetworkDebugSessionView.module.css";

// ── 类型 ───────────────────────────────────────────

export interface PacketRow {
  id: number;
  time: string;
  direction: "RX" | "TX";
  /** 来源地址（RX）或目标地址（TX） */
  peerLabel: string;
  length: number;
  hex: string;
  text: string;
}

type TcpDisplayMode = "dual" | "text" | "hex";

interface NetworkViewState {
  transport: "tcp" | "udp";
  role: "client" | "server";
  /** 会话字符编码（文本栏解码；连接时确定） */
  encoding: string;
  /** TCP 对端帧（DualLine 列表，按对端隔离） */
  frames: Record<string, DualLine[]>;
  /** UDP 逐数据报网格（会话级时间线） */
  packets: PacketRow[];
  /** UDP 会话级累计 RX 字节（无对端模型，直接驱动状态栏） */
  rxBytes: number;
  /** UDP 会话级累计 TX 字节（无对端模型，直接驱动状态栏） */
  txBytes: number;
  /** UDP 会话级累计 RX 报文数（驱动状态栏报文计数） */
  rxPackets: number;
  /** UDP 会话级累计 TX 报文数 */
  txPackets: number;
}

// ── 常量 ───────────────────────────────────────────

const FRAME_BUFFER_LINES = 2000;
const PACKET_BUFFER_ROWS = 2000;
const FRAME_TIMEOUT_MS = 50;

// ── 监听器依赖注册表 ─────────────────────────────────
// initListeners 由 usePluginSessionStore 每个会话只注册一次，闭包捕获首次挂载的
// refs；视图切换重挂载后旧 refs 的 .current 不再刷新，导致监听器用陈旧对端列表
// 路由数据（服务器数据区为空）。每次渲染把最新 refs 写入本注册表，监听器在事件
// 到达时按 sessionId 实时读取，彻底消除重挂载造成的闭包失效。
const listenerDeps: Record<string, InitDeps> = {};

// ── 纯函数辅助 ─────────────────────────────────────

function nowTime(): string {
  const d = new Date();
  const p = (n: number, w = 2) => String(n).padStart(w, "0");
  return `${p(d.getHours())}:${p(d.getMinutes())}:${p(d.getSeconds())}.${p(d.getMilliseconds(), 3)}`;
}

function decodeText(bytes: Uint8Array, encoding: string, stream = false): string {
  try {
    return new TextDecoder(encoding).decode(bytes, stream ? { stream: true } : undefined);
  } catch {
    return new TextDecoder("utf-8").decode(bytes);
  }
}

/** 追加一帧到对端帧列表（读取最新 state 计算 id，避免同快照连续调用产生重复 key） */
function pushFrame(
  api: SessionStoreApi<NetworkViewState>,
  peerId: string,
  direction: "RX" | "TX",
  data: Uint8Array,
  encoding: string,
  decodedOverride?: string,
) {
  if (data.length === 0) return;
  const decoded = decodedOverride !== undefined
    ? decodedOverride
    : decodeText(data, encoding, direction === "RX");
  const base = { ...dataToDualLine(data, direction), text: normalizeDecodedText(decoded) };
  if (base.text.length === 0 && base.hex.length === 0) return;
  const fresh = api.getState();
  const prev = fresh.frames[peerId] ?? [];
  const id = prev.length > 0 ? prev[prev.length - 1].id + 1 : 1;
  const list = [...prev, { ...base, id }];
  if (list.length > FRAME_BUFFER_LINES) list.splice(0, list.length - FRAME_BUFFER_LINES);
  api.setState({ frames: { ...fresh.frames, [peerId]: list } });
}

/** 追加一个 UDP 数据报行（读取最新 state 计算 id，保证会话级时间线 id 单调唯一） */
function pushPacket(
  api: SessionStoreApi<NetworkViewState>,
  row: Omit<PacketRow, "id">,
) {
  const fresh = api.getState();
  const prev = fresh.packets;
  const id = prev.length > 0 ? prev[prev.length - 1].id + 1 : 1;
  const list = [...prev, { ...row, id }];
  if (list.length > PACKET_BUFFER_ROWS) list.splice(0, list.length - PACKET_BUFFER_ROWS);
  api.setState({
    packets: list,
    rxBytes: fresh.rxBytes + (row.direction === "RX" ? row.length : 0),
    txBytes: fresh.txBytes + (row.direction === "TX" ? row.length : 0),
    rxPackets: fresh.rxPackets + (row.direction === "RX" ? 1 : 0),
    txPackets: fresh.txPackets + (row.direction === "TX" ? 1 : 0),
  });
}

// ── 会话持久状态 ───────────────────────────────────

function createState(
  transport: "tcp" | "udp" = "tcp",
  role: "client" | "server" = "client",
  encoding = "utf-8",
): NetworkViewState {
  return {
    transport,
    role,
    encoding,
    frames: {},
    packets: [],
    rxBytes: 0,
    txBytes: 0,
    rxPackets: 0,
    txPackets: 0,
  };
}

// ── 主视图 ─────────────────────────────────────────

interface Props {
  sessionId: string;
}

export default function NetworkDebugSessionView({ sessionId }: Props) {
  const { t } = useTranslation();
  const { state: sessionState, selectNetworkPeer, getNetworkPeers, mergeNetworkPeers, registerNetworkUdpSource, subscribeDataSent, subscribeNetworkManualSent, updateSessionStats } = useSession();

  // 容器会话参数（连接时确定，稳定）：transport / role / encoding / data_mode
  const containerTab = sessionState.tabs.find(tab => tab.id === sessionId);
  const transport = (containerTab?.params?.transport as "tcp" | "udp") ?? "tcp";
  const role = (containerTab?.params?.role as "client" | "server") ?? "client";
  const encoding = (containerTab?.params?.encoding as string) ?? "utf-8";
  /** 数据模式来自连接参数（与串口一致，会话内不可切换） */
  const displayMode = (containerTab?.params?.data_mode as TcpDisplayMode | undefined) ?? "dual";

  // 对端数据源（SessionContext；ref 镜像供全局监听器读取最新列表）
  const peers = sessionState.networkPeers[sessionId] ?? [];
  const selectedPeerId = sessionState.selectedNetworkPeer[sessionId] ?? null;
  const peersRef = useRef<NetworkPeerEntry[]>([]);
  peersRef.current = peers;

  // 分帧器（TCP 流式对端，按对端独立）；组件与 init 闭包共享同一 Map 实例
  const framersRef = useRef<Map<string, StreamFramer>>(new Map());
  // 流式解码器（TCP RX 文本栏，按对端缓存，处理跨帧多字节字符）
  const decodersRef = useRef<Map<string, TextDecoder>>(new Map());

  // 每次渲染刷新监听器依赖注册表：initListeners 只注册一次，事件到达时按
  // sessionId 读取此处的最新 refs（修复视图重挂载后旧闭包持有陈旧 refs）
  listenerDeps[sessionId] = { getNetworkPeers, peersRef, framersRef, decodersRef, subscribeDataSent, subscribeNetworkManualSent, registerNetworkUdpSource };

  const { state: snap, api } = usePluginSessionStore<NetworkViewState>(sessionId, {
    createState: () => createState(transport, role, encoding),
    init: (api) => initListeners(api, { getNetworkPeers, peersRef, framersRef, decodersRef, subscribeDataSent, subscribeNetworkManualSent, registerNetworkUdpSource }),
    keepAlive: true,
    onSessionDisconnected: () => {
      // 容器断开：清理分帧/解码器（对端注册由 SessionContext 级联清理）
      for (const f of framersRef.current.values()) f.dispose();
      framersRef.current.clear();
      for (const d of decodersRef.current.values()) { try { d.decode(); } catch { /* 冲刷残留 */ } }
      decodersRef.current.clear();
      return { frames: {}, packets: [], rxBytes: 0, txBytes: 0, rxPackets: 0, txPackets: 0 };
    },
    getStatus: async (sid, _api) => {
      try {
        const list = await invoke<any[]>("list_network_peers", { sessionId: sid });
        // 对端条目已迁至 SessionContext：快照合并进 context（保留既有统计）
        mergeNetworkPeers(sid, list.map((p) => ({
          peerId: p.peer_id,
          name: p.name,
          addr: p.addr,
          localAddr: p.local_addr,
          state: p.state === "connected" ? "connected" : "disconnected",
          txBytes: p.tx_bytes ?? 0,
          rxBytes: p.rx_bytes ?? 0,
        })));
        return undefined;
      } catch {
        return undefined;
      }
    },
  });

  // 会话参数（连接时确定）与 store 同步：重连/编辑后参数变化时更新
  useEffect(() => {
    if (snap.transport !== transport || snap.role !== role || snap.encoding !== encoding) {
      api.setState({ transport, role, encoding });
    }
  }, [transport, role, encoding, snap.transport, snap.role, snap.encoding, api]);

  // TCP client 模式：自动选中唯一对端（无对端列表，无需手动点选）。UDP 无对端。
  useEffect(() => {
    if (transport !== "tcp" || role !== "client") return;
    if (peers.length === 1 && selectedPeerId !== peers[0].peerId) {
      selectNetworkPeer(sessionId, peers[0].peerId);
    }
  }, [transport, role, peers, selectedPeerId, sessionId, selectNetworkPeer]);

  // 对端被移除（clearNetworkPeer）时清理其残留帧
  useEffect(() => {
    const ids = new Set(peers.map(p => p.peerId));
    const frames = { ...snap.frames };
    let changed = false;
    for (const k of Object.keys(frames)) {
      if (!ids.has(k)) { delete frames[k]; changed = true; }
    }
    if (changed) api.setState({ frames });
  }, [peers, snap.frames, api]);

  // ── 会话统计汇总 → 容器状态栏 ──
  // UDP 无对端：由视图内累计的 rx/tx 字节直接驱动；TCP 由对端统计聚合。
  useEffect(() => {
    if (snap.transport === "udp") {
      updateSessionStats(sessionId, snap.txBytes, snap.rxBytes, snap.rxPackets, snap.txPackets);
    } else {
      const tx = peers.reduce((s, p) => s + p.txBytes, 0);
      const rx = peers.reduce((s, p) => s + p.rxBytes, 0);
      updateSessionStats(sessionId, tx, rx);
    }
  }, [snap.transport, snap.txBytes, snap.rxBytes, snap.rxPackets, snap.txPackets, peers, sessionId, updateSessionStats]);

  // ── 渲染数据 ─────────────────────────────────────

  // TCP client 是单对端会话：渲染期直接以唯一对端为有效选中。
  // 避免 stored selection 为空/陈旧（切换瞬间、重连替换对端）时闪一帧"未选择对端"，
  // 自动选择 effect 随后会同步 stored selection，保持发送栏等消费方一致。
  const effectiveSelectedPeerId = role === "client" && peers.length === 1
    ? peers[0].peerId
    : selectedPeerId;
  const selectedPeer = peers.find(p => p.peerId === effectiveSelectedPeerId) ?? null;
  const containerConnected = containerTab?.state === "connected" || containerTab?.state === "transferring";
  const lifecycleHint = t("session.connectToViewContent", "连接后显示会话内容");

  return (
    <div className={styles.container}>
      {/* UDP：未连接时显示与终端会话一致的标准空状态；连接后展示全部来源时间线报文网格 */}
      {snap.transport === "udp" && (
        !containerConnected ? (
          <div className={styles.emptyState}>
            <Icon name="connection" size="xl" className={styles.emptyIcon} />
            <div>{lifecycleHint}</div>
          </div>
        ) : (
          <div className={styles.dataArea}>
            <UdpPacketGrid rows={snap.packets} />
          </div>
        )
      )}

      {/* TCP：未连接时显示与终端会话一致的标准空状态；连接后按对端状态渲染 */}
      {snap.transport === "tcp" && (
        !containerConnected ? (
          <div className={styles.emptyState}>
            <Icon name="connection" size="xl" className={styles.emptyIcon} />
            <div>{lifecycleHint}</div>
          </div>
        ) : peers.length === 0 ? (
          <div className={styles.emptyState}>
            <Icon name="globe" size="xl" className={styles.emptyIcon} />
            <div>{snap.role === "server" ? t("network.waitingClients") : t("network.noPeers")}</div>
          </div>
        ) : !selectedPeer ? (
          <div className={styles.emptyState}>
            <Icon name="globe" size="xl" className={styles.emptyIcon} />
            <div>{t("network.noSelectedPeer")}</div>
            <div className={styles.emptyHint}>{t("network.selectPeerHint")}</div>
          </div>
        ) : (
          <div className={styles.dataArea}>
            {displayMode === "dual" ? (
              <DualPane lines={snap.frames[selectedPeer.peerId] ?? []} />
            ) : displayMode === "text" ? (
              <TcpTextList lines={snap.frames[selectedPeer.peerId] ?? []} />
            ) : (
              <TcpHexList lines={snap.frames[selectedPeer.peerId] ?? []} />
            )}
          </div>
        )
      )}
    </div>
  );
}

// ── TCP Text / Hex 单栏视图（从 DualLine 数据渲染） ──

function TcpTextList({ lines }: { lines: DualLine[] }) {
  const { scrollRef, isAtBottom, handleScroll, scrollToBottom } = useAutoScroll<HTMLDivElement>(lines);
  return (
    <>
      <div className={styles.singleList} ref={scrollRef} onScroll={handleScroll}>
        {lines.map(l => (
          <div key={l.id} className={l.direction === "TX" ? styles.txLine : styles.rxLine}>
            <span className={styles.singleMeta}>[{l.direction}][{l.timestamp}]</span>
            <span className={styles.singleText}>{l.text}</span>
          </div>
        ))}
      </div>
      <ScrollToBottomButton visible={!isAtBottom} onClick={scrollToBottom} />
    </>
  );
}

function TcpHexList({ lines }: { lines: DualLine[] }) {
  const { scrollRef, isAtBottom, handleScroll, scrollToBottom } = useAutoScroll<HTMLDivElement>(lines);
  return (
    <>
      <div className={styles.singleList} ref={scrollRef} onScroll={handleScroll}>
        {lines.map(l => (
          <div key={l.id} className={l.direction === "TX" ? styles.txLine : styles.rxLine}>
            <span className={styles.singleMeta}>[{l.direction}][{l.timestamp}]</span>
            <span className={styles.singleHex}>{l.hex}</span>
          </div>
        ))}
      </div>
      <ScrollToBottomButton visible={!isAtBottom} onClick={scrollToBottom} />
    </>
  );
}

// ── 工具函数 ───────────────────────────────────────

function bytesToHex(bytes: Uint8Array): string {
  const parts: string[] = [];
  for (let i = 0; i < bytes.length; i++) {
    if (i > 0) parts.push(" ");
    if (i > 0 && i % 8 === 0) parts.push(" ");
    parts.push(bytes[i].toString(16).padStart(2, "0"));
  }
  return parts.join("");
}

// ── 事件监听（会话级，仅注册一次） ─────────────────

interface InitDeps {
  /** 读取会话当前对端列表（读 SessionContext stateRef，非活跃会话也始终新鲜） */
  getNetworkPeers: (containerId: string) => NetworkPeerEntry[];
  peersRef: React.MutableRefObject<NetworkPeerEntry[]>;
  framersRef: React.MutableRefObject<Map<string, StreamFramer>>;
  decodersRef: React.MutableRefObject<Map<string, TextDecoder>>;
  /** 对端发送通知（TX 回显） */
  subscribeDataSent: (callback: (sessionId: string, data: Uint8Array) => void) => () => void;
  /** UDP 发送通知（TX 回显） */
  subscribeNetworkManualSent: (callback: (containerId: string, target: string, bytes: Uint8Array) => void) => () => void;
  /** 记录 UDP RX 来源地址（发送栏快捷回发用） */
  registerNetworkUdpSource: (containerId: string, addr: string) => void;
}

function initListeners(
  api: SessionStoreApi<NetworkViewState>,
  initDeps: InitDeps,
) {
  const unData = listen<any>("session-data", (event) => {
    const e = event.payload;
    const sid = e.session_id;
    const s = api.getState();
    // 依赖统一从注册表（组件每次渲染刷新）读取，initDeps 兜底。
    const deps = listenerDeps[api.sessionId] ?? initDeps;
    const { getNetworkPeers, framersRef, decodersRef } = deps;
    const bytes = e.data_b64
      ? base64ToBytes(e.data_b64)
      : new Uint8Array(e.data ?? []);
    if (bytes.length === 0) return;

    if (s.transport === "udp") {
      // UDP 无对端：session_id = 容器，source_addr 为来源地址，逐报文记网格
      if (sid !== api.sessionId) return;
      const sourceAddr = (e.source_addr as string) || sid;
      deps.registerNetworkUdpSource(api.sessionId, sourceAddr);
      pushPacket(api, {
        time: nowTime(),
        direction: "RX",
        peerLabel: sourceAddr,
        length: bytes.length,
        hex: bytesToHex(bytes),
        text: decodeText(bytes, s.encoding),
      });
      return;
    }

    // TCP：session_id = 对端 UUID，需匹配对端再分帧。
    // 对端匹配读 SessionContext stateRef（getNetworkPeers 始终新鲜）：
    // 非活跃会话视图不渲染，refs 会冻结，不能作为后台收发的路由依据。
    const peer = getNetworkPeers(api.sessionId).find(p => p.peerId === sid);
    if (!peer) return;

    // 分帧（分隔符 + 超时）→ 帧追加
    let framer = framersRef.current.get(sid);
    if (!framer) {
      framer = new StreamFramer(FRAME_TIMEOUT_MS);
      framersRef.current.set(sid, framer);
    }
    // 按对端缓存的流式解码器：处理跨帧被切断的多字节字符（如 GBK 双字节）
    const enc = s.encoding;
    let dec = decodersRef.current.get(sid);
    if (!dec) {
      try { dec = new TextDecoder(enc); } catch { dec = new TextDecoder("utf-8"); }
      decodersRef.current.set(sid, dec);
    }
    framer.push(bytes, (frame) => {
      const text = dec!.decode(frame, { stream: true });
      pushFrame(api, sid, "RX", frame, enc, text);
    });
  });

  // ── TX 回显（TCP 对端发送 → 追加 TX 行；群发扇出由 SessionContext.sendData 统一处理） ──
  // 与 RX 一样注册在会话级持久监听器中（keepAlive=true）：视图切换/卸载后仍持续追加，
  // 保证后台发送期间的 TX 回显不丢失。UDP 的 TX 回显走 subscribeNetworkManualSent。
  const unDataSent = initDeps.subscribeDataSent((sid, bytes) => {
    const s = api.getState();
    if (s.transport === "udp") return;
    const peer = initDeps.getNetworkPeers(api.sessionId).find(p => p.peerId === sid);
    if (!peer) return;
    pushFrame(api, sid, "TX", bytes, s.encoding);
  });

  // ── UDP 发送回显（server 手动目标 / client 固定远端，经 sendNetworkData 触发） ──
  const unManualSent = initDeps.subscribeNetworkManualSent((cid, target, bytes) => {
    if (cid !== api.sessionId) return;
    const s = api.getState();
    pushPacket(api, {
      time: nowTime(),
      direction: "TX",
      peerLabel: target,
      length: bytes.length,
      hex: bytesToHex(bytes),
      text: decodeText(bytes, s.encoding),
    });
  });

  return Promise.all([unData, unDataSent, unManualSent]);
}

// ── Base64 解码（与后端 data_batcher::base64_encode 配对） ──

function base64ToBytes(b64: string): Uint8Array {
  const binary = atob(b64);
  const bytes = new Uint8Array(binary.length);
  for (let i = 0; i < binary.length; i++) bytes[i] = binary.charCodeAt(i);
  return bytes;
}
