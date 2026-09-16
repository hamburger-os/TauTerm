/**
 * iperf Session 主视图
 *
 * 布局：顶部版本展示（iperf2 / iperf3）
 *       左侧：服务端面板（仅监听参数）+ 客户端面板（全部测试参数）
 *       右侧：测试记录列表 + 选中记录详情（日志/汇总/图表）
 *
 * content_type: "custom" → CustomRenderer → IperfSessionView
 *
 * 状态由共享 hook `usePluginSessionStore` 管理（会话级持久 store）：
 * - 监听器按会话注册，会话断开时（keepAlive=false）注销并重注册
 * - `session-disconnected` 全局只监听一次，按会话分发
 * - 看门狗（done 事件兜底）触发时自删条目
 */
import { useCallback, useEffect, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { useTranslation } from "react-i18next";
import { useSession } from "../../context/SessionContext";
import {
  usePluginSessionStore,
  type SessionStoreApi,
} from "../../hooks/usePluginSessionStore";
import IperfServerPanel from "./IperfServerPanel";
import IperfClientPanel from "./IperfClientPanel";
import IperfRecordList from "./IperfRecordList";
import IperfRecordDetail from "./IperfRecordDetail";
import {
  formatIntervalLine,
  IPERF_HEADER_TCP,
  IPERF_HEADER_UDP,
} from "./iperf-utils";
import {
  intervalFromPayload,
  paramsFromBackend,
  summaryFromPayload,
  type IperfDirection,
  type IperfDoneEvent,
  type IperfDynamicParamsBackend,
  type IperfIntervalEvent,
  type IperfParams,
  type IperfProtocolStr,
  type IperfRecord,
  type IperfRoleStr,
  type IperfServerStatusEvent,
  type IperfTestStartedEvent,
  type IperfVersionStr,
  type SessionConnectedEvent,
} from "./iperf-events";
import styles from "./IperfSessionView.module.css";

// ═══════════════════════════════════════════════════════════════════
// 会话缓存状态
// ═══════════════════════════════════════════════════════════════════

interface CachedState {
  version: IperfVersionStr;
  serverRunning: boolean;
  listenAddr: string;
  listenIp: string;
  listenPort: number;
  clientParams: IperfParams;
  targetHost: string;
  records: IperfRecord[];
  selectedRecordId: string | null;
  clientTestRunning: boolean;
  serverTestRunning: boolean;
  clientError: string | null;
  serverError: string | null;
  loaded: boolean;
  /** 服务端测试时长提示（started 事件携带；看门狗与惰性 rev 记录使用） */
  serverTestDurationSecs: number | null;
  /** 服务端测试是否双向（-d/-r；总时长翻倍） */
  serverTestBidirectional: boolean;
}

function createState(): CachedState {
  return {
    version: "iperf2",
    serverRunning: false,
    listenAddr: "",
    listenIp: "0.0.0.0",
    listenPort: 5001,
    clientParams: { ...DEFAULT_PARAMS },
    targetHost: "",
    records: [],
    selectedRecordId: null,
    clientTestRunning: false,
    serverTestRunning: false,
    clientError: null,
    serverError: null,
    loaded: false,
    serverTestDurationSecs: null,
    serverTestBidirectional: false,
  };
}

const DEFAULT_PARAMS: IperfParams = {
  protocol: "tcp",
  durationSecs: 10,
  port: 5001,
  parallelStreams: 1,
  reportIntervalSecs: 1,
  bandwidthBps: null,
  bidirectional: false,
  tradeoff: false,
  windowSize: null,
  reverse: false,
  bidir: false,
  omitSecs: 0,
};

/** 记录 id 全局自增（跨会话唯一） */
let recordSeq = 0;

// ═══════════════════════════════════════════════════════════════════
// 记录配对（纯函数）
// ═══════════════════════════════════════════════════════════════════

/** 当前进行中的记录（按角色 + 方向 + 协议查找：-d/-r 下同角色 fwd/rev 并发
 * running；同会话 TCP/UDP 服务器记录共存时靠 protocol 区分，不按 index 误配） */
function activeRecord(
  s: CachedState,
  role: IperfRoleStr,
  direction: IperfDirection,
  protocol?: IperfProtocolStr
): IperfRecord | undefined {
  return s.records.find(
    (r) =>
      r.role === role &&
      r.direction === direction &&
      r.status === "running" &&
      (protocol === undefined || r.protocol === protocol)
  );
}

/**
 * 按事件配对记录：优先 seq 精确匹配（UDP 服务端多路复用下并发记录
 * 需按 seq 归位），无 seq 的事件（客户端/TCP 服务端）回退活跃记录。
 * protocol 用于区分同角色下并存的 TCP/UDP 记录，避免汇总被误套。
 */
function matchRecord(
  s: CachedState,
  role: IperfRoleStr,
  direction: IperfDirection,
  seq?: number,
  protocol?: IperfProtocolStr
): IperfRecord | undefined {
  if (typeof seq === "number") {
    const bySeq = s.records.find(
      (r) =>
        r.role === role &&
        r.direction === direction &&
        r.status === "running" &&
        r.seq === seq
    );
    if (bySeq) return bySeq;
  }
  return activeRecord(s, role, direction, protocol);
}

/** 看门狗时长：双向测试（-d/-r）总时长约为 fwd 的两倍 */
function watchdogMs(durationSecs: number, bidirectional: boolean): number {
  return Math.max(15_000, (durationSecs + 10) * 1000) * (bidirectional ? 2 : 1);
}

/** 未知时长（量模式/协议未知）的服务端测试看门狗上限：仅兜底真挂起，
 * 不得误伤合法长测试（UI 允许最长 3600s） */
const UNKNOWN_DURATION_WATCHDOG_MS = 3_600_000;

function newRecordId(): string {
  return `${Date.now()}-${++recordSeq}`;
}

function headerLine(protocol: IperfProtocolStr): string {
  return protocol === "udp" ? IPERF_HEADER_UDP : IPERF_HEADER_TCP;
}

// ═══════════════════════════════════════════════════════════════════
// 事件监听（会话注册一次；断连时由 hook 注销，重连重注册）
// ═══════════════════════════════════════════════════════════════════

async function initListeners(
  sessionId: string,
  api: SessionStoreApi<CachedState>
) {
  const unServerStatus = await listen<IperfServerStatusEvent>(
    "iperf-server-status",
    ({ payload: e }) => {
      if (e.session_id !== sessionId) return;
      const s = api.getState();
      api.setState({
        serverRunning: !!e.running,
        listenAddr: e.listen_addr || s.listenAddr,
        // 启动成功清空历史错误；停止事件可携带 error（如端口被占用绑定失败）
        serverError: e.running ? null : (e.error ?? s.serverError),
      });
    }
  );

  const unStarted = await listen<IperfTestStartedEvent>(
    "iperf-test-started",
    ({ payload: e }) => {
      if (e.session_id !== sessionId) return;
      try {
        const s = api.getState();
        const role: IperfRoleStr = e.role;
        const direction: IperfDirection = e.direction === "rev" ? "rev" : "fwd";
        // pending（iperf3 服务端待命/接待中）：不建记录、不自动选中——记录在
        // done 事件时新建；仅置服务端运行标志（与 done 清除对应），不设看门狗
        // （空闲服务端可无限等待客户端，任何固定超时都会把正常等待误标失败）
        if (e.pending === true) {
          api.setState({ serverTestRunning: true });
          return;
        }
        const protocol: IperfProtocolStr = e.protocol === "udp" ? "udp" : "tcp";
        const id = newRecordId();
        const record: IperfRecord = {
          id,
          role,
          direction,
          version: s.version,
          protocol,
          startTime: Date.now(),
          status: "running",
          summary: null,
          intervals: [],
          // 并发记录配对键（UDP 服务端多路复用：started/done/interval 按 seq 归位）
          seq: typeof e.seq === "number" ? e.seq : undefined,
          // 标准 iperf2 表头（UDP 追加抖动/丢包列）
          logLines: [headerLine(protocol)],
        };
        const durationSecs =
          typeof e.duration_secs === "number" ? e.duration_secs : null;
        const bidirectional = !!e.bidirectional;
        api.setState({
          records: [record, ...s.records].slice(0, 50),
          selectedRecordId: id,
          clientTestRunning: role === "client" || s.clientTestRunning,
          serverTestRunning: role === "server" || s.serverTestRunning,
          serverTestDurationSecs:
            role === "server" ? durationSecs : s.serverTestDurationSecs,
          serverTestBidirectional:
            role === "server" ? bidirectional : s.serverTestBidirectional,
        });

        // done 看门狗：后端保证 done 一定发出（含 panic 兜底）；万一仍超时
        // （任务挂起等极端情况），强制复位记录与运行状态。
        // 服务端未知时长（量模式 duration=0 或协议未知）不套固定短超时——
        // 15s/120s 会把合法长测试中途标失败，用 UI 上限时长兜底真挂起
        const timeoutMs =
          role === "client"
            ? watchdogMs(s.clientParams.durationSecs,
                s.clientParams.bidirectional || s.clientParams.tradeoff)
            : durationSecs != null && durationSecs > 0
              ? watchdogMs(durationSecs, bidirectional)
              : UNKNOWN_DURATION_WATCHDOG_MS;
        api.setWatchdog(id, () => {
          const cur = api.getState();
          const running = cur.records.find(
            (r) => r.id === id && r.status === "running"
          );
          if (!running) return;
          api.setState({
            records: cur.records.map((r) =>
              r.id === id && r.status === "running"
                ? { ...r, status: "failed" as const, error: "watchdog: test timed out" }
                : r
            ),
            // 按角色复位运行标志：客户端/服务端测试互不串扰（对齐 done 处理）
            clientTestRunning:
              running.role === "client" ? false : cur.clientTestRunning,
            serverTestRunning:
              running.role === "server" ? false : cur.serverTestRunning,
          });
        }, timeoutMs);
      } catch (err) {
        // 防御：started 处理异常不阻止后续事件（看门狗由 done 兜底）
        console.error("[iperf] started 处理异常:", err);
      }
    }
  );

  const unInterval = await listen<IperfIntervalEvent>(
    "iperf-interval-report",
    ({ payload: e }) => {
      if (e.session_id !== sessionId) return;
      const s = api.getState();
      // 按事件角色路由：自测模式（客户端/服务端记录同时在 running）下服务端
      // 上报必须落到服务端记录
      const role: IperfRoleStr = e.role;
      const direction: IperfDirection = e.direction === "rev" ? "rev" : "fwd";
      // 流标签：-P>1 时区间为跨流聚合，对齐标准 iperf2 用 [SUM]；服务端记录
      // 对外部客户端的 -P 未知，按单流 [  1]
      const label =
        role === "client" && s.clientParams.parallelStreams > 1 ? "SUM" : "1";
      const proto: IperfProtocolStr = e.protocol === "udp" ? "udp" : "tcp";
      let target = matchRecord(s, role, direction, e.seq, proto);
      // 服务端实时流：iperf3 pending 不建记录，首个区间 = 客户端已接入并开跑
      // → 惰性新建 running 记录（后续区间/done 正常归位）。serverTestRunning
      // 守卫：done 之后的迟到区间不造幽灵记录。协议与方向来自事件（后端补发）
      let newRecord: IperfRecord | null = null;
      if (!target && role === "server" && s.serverTestRunning) {
        const protocol: IperfProtocolStr = e.protocol === "udp" ? "udp" : "tcp";
        const id = newRecordId();
        newRecord = {
          id,
          role,
          direction,
          version: s.version,
          protocol,
          startTime: Date.now(),
          status: "running",
          summary: null,
          intervals: [],
          logLines: [headerLine(protocol)],
        };
        target = newRecord;
        // 惰性记录同样设看门狗（done 兜底；rev 相晚于 fwd 结束）
        api.setWatchdog(id, () => {
          const cur = api.getState();
          api.setState({
            records: cur.records.map((r) =>
              r.id === id && r.status === "running"
                ? { ...r, status: "failed" as const, error: "watchdog: test timed out" }
                : r
            ),
            serverTestRunning: false,
          });
        }, s.serverTestDurationSecs && s.serverTestDurationSecs > 0
          ? watchdogMs(s.serverTestDurationSecs, s.serverTestBidirectional)
          : UNKNOWN_DURATION_WATCHDOG_MS);
      }
      if (!target) return; // 无运行记录可归位（迟到区间等）——丢弃
      const interval = intervalFromPayload(e);
      const records = (
        newRecord ? [newRecord, ...s.records].slice(0, 50) : s.records
      ).map((r) =>
        r.id === target.id
          ? {
              ...r,
              intervals: [...r.intervals, interval],
              logLines: [...r.logLines, formatIntervalLine(interval, label)],
            }
          : r
      );
      api.setState({
        records,
        selectedRecordId: newRecord ? newRecord.id : s.selectedRecordId,
      });
    }
  );

  const unDone = await listen<IperfDoneEvent>(
    "iperf-test-done",
    ({ payload: e }) => {
      if (e.session_id !== sessionId) return;
      try {
        const s = api.getState();
        const role: IperfRoleStr = e.role;
        const direction: IperfDirection = e.direction === "rev" ? "rev" : "fwd";
        // 累计行流标签（对齐标准 iperf2：-P>1 为 [SUM]，否则 [  1]）
        const doneLabel =
          role === "client" && s.clientParams.parallelStreams > 1 ? "SUM" : "1";
        const summary = summaryFromPayload(e.summary);
        const proto: IperfProtocolStr = e.protocol === "udp" ? "udp" : "tcp";
        let target = matchRecord(s, role, direction, e.seq, proto);
        // 无运行记录时按 summary 直接新建完成/失败记录：
        // - iperf3 服务端 pending 轮次（无区间）
        // - rev 相零字节（无区间 → 无惰性记录，done 仍须呈现）
        if (
          !target &&
          e.success &&
          role === "client" &&
          direction === "fwd"
        ) {
          // Stop 乐观更新与在途 done 调和：成功 done 到达时记录可能已被
          // handleClientStop 标为"已停止"，将其改回完成态（真实完成的测试
          // 不应显示为停止、最终摘要不应丢失）
          target = s.records.find(
            (r) =>
              r.role === role &&
              r.direction === direction &&
              r.protocol === proto &&
              r.status === "failed" &&
              r.error === "已停止"
          );
        }
        if (!target && (role === "server" || direction === "rev")) {
          const protocol: IperfProtocolStr =
            summary?.protocol ?? (e.protocol === "udp" ? "udp" : "tcp");
          const id = newRecordId();
          const doneLine = summary
            ? formatIntervalLine(
                {
                  startSecs: 0,
                  endSecs: summary.durationSecs,
                  transferredBytes: summary.totalBytes,
                  bandwidthBps: summary.avgBandwidthBps,
                  jitterMs: summary.jitterMs,
                  lostPackets: summary.lostPackets,
                  totalPackets: summary.totalPackets,
                  lostPercent: summary.lostPercent,
                },
                doneLabel
              )
            : null;
          const record: IperfRecord = {
            id,
            role,
            direction,
            version: s.version,
            protocol,
            // 无 started 时间戳：按测试时长回推展示（近似起点）
            startTime:
              Date.now() - Math.round((summary?.durationSecs ?? 0) * 1000),
            status: e.success ? ("completed" as const) : ("failed" as const),
            error: e.error ?? undefined,
            warning: e.warning ?? undefined,
            summary,
            intervals: summary?.intervals ?? [],
            // 表头 + 逐区间行 + 累计行（与客户端记录内容一致）
            logLines: [
              headerLine(protocol),
              ...(summary?.intervals.map((i) => formatIntervalLine(i, doneLabel)) ??
                []),
              ...(doneLine ? [doneLine] : []),
            ],
          };
          api.setState({
            records: [record, ...s.records].slice(0, 50),
            selectedRecordId: id,
            serverTestRunning: false,
          });
          return; // 该记录无看门狗（完成即终态）
        }
        if (!target) return; // 客户端无运行记录（如看门狗已先行复位）——丢弃
        api.setState({
          records: s.records.map((r) => {
            if (r.id !== target.id) return r;
            return {
              ...r,
              status: e.success ? ("completed" as const) : ("failed" as const),
              error: e.error ?? undefined,
              warning: e.warning ?? undefined,
              summary,
              // 汇总补全：summary 里的 intervals 与运行中收集的一致（以后端为准）
              intervals:
                summary?.intervals?.length ? summary.intervals : r.intervals,
              // 标准累计行（对齐 iperf2 末行）：`[  1] 0.00-10.00 sec  29.3 GBytes  25.2 Gbits/sec`
              logLines: summary
                ? [
                    ...r.logLines,
                    formatIntervalLine(
                      {
                        startSecs: 0,
                        endSecs: summary.durationSecs,
                        transferredBytes: summary.totalBytes,
                        bandwidthBps: summary.avgBandwidthBps,
                        jitterMs: summary.jitterMs,
                        lostPackets: summary.lostPackets,
                        totalPackets: summary.totalPackets,
                        lostPercent: summary.lostPercent,
                      },
                      doneLabel
                    ),
                  ]
                : r.logLines,
            };
          }),
          clientTestRunning: role === "client" ? false : s.clientTestRunning,
          serverTestRunning: role === "server" ? false : s.serverTestRunning,
        });
        api.clearWatchdog(target.id);
      } catch (err) {
        // 防御：异常不阻止状态恢复（看门狗仍会兜底复位）
        console.error("[iperf] done 处理异常:", err);
      }
    }
  );

  const unConnected = await listen<SessionConnectedEvent>(
    "session-connected",
    ({ payload: e }) => {
      if (e.session_id !== sessionId) return;
      const s = api.getState();
      const p = e.params || {};
      const version: IperfVersionStr =
        p.version === "iperf3" ? "iperf3" : "iperf2";
      const listenPort: number =
        typeof p.listen_port === "number"
          ? p.listen_port
          : version === "iperf2"
            ? 5001
            : 5201;
      api.setState({
        version,
        listenIp: typeof p.listen_ip === "string" ? p.listen_ip : s.listenIp,
        listenPort,
        listenAddr:
          typeof p.listen_ip === "string"
            ? `${p.listen_ip}:${listenPort}`
            : s.listenAddr,
        // 注意：不在此覆写 clientParams.port——客户端目标端口(-p)与监听端口
        // 是独立字段，唯一事实源是后端 dynamic_params（loadBackendStatus 恢复）
        loaded: true,
      });
    }
  );

  return [unServerStatus, unStarted, unInterval, unDone, unConnected];
}

/** 后端实时状态加载（getStatus 选项；挂载时执行一次） */
async function loadBackendStatus(
  sessionId: string,
  api: SessionStoreApi<CachedState>
): Promise<Partial<CachedState> | undefined> {
  try {
    const res = await invoke<{
      server_running: boolean;
      client_test_running: boolean;
      listen_addr: string | null;
      listen_port: number | null;
      dynamic_params: IperfDynamicParamsBackend | null;
    }>("iperf_get_status", { sessionId });
    const s = api.getState();
    const patch: Partial<CachedState> = {
      serverRunning: !!res.server_running,
      // 客户端与服务端运行标志独立（后端已拆分）
      clientTestRunning: !!res.client_test_running,
      loaded: true,
    };
    // 已连接（侧通道存在 → listen_addr 非空）才应用后端动态参数；
    // 断连时后端返回默认值，保留 store/会话参数中的用户选择
    if (res.listen_addr != null) {
      const listenPort: number = res.listen_port ?? s.listenPort;
      patch.listenPort = listenPort;
      patch.listenAddr = `${res.listen_addr}:${listenPort}`;
      patch.clientParams = res.dynamic_params
        ? {
            ...paramsFromBackend(res.dynamic_params),
            port: res.dynamic_params.port ?? s.clientParams.port,
          }
        : s.clientParams;
    }
    return patch;
  } catch {
    return { loaded: true };
  }
}

// ═══════════════════════════════════════════════════════════════════
// 组件
// ═══════════════════════════════════════════════════════════════════

interface Props {
  sessionId: string;
}

export default function IperfSessionView({ sessionId }: Props) {
  const { t } = useTranslation();
  const { state: sessionState } = useSession();

  const { state, api } = usePluginSessionStore<CachedState>(sessionId, {
    createState,
    init: (api) => initListeners(sessionId, api),
    keepAlive: false,
    onSessionDisconnected: (s) => ({
      // 断开仅停止服务端监听：保留客户端参数与历史记录，重置服务端状态
      //（客户端自给自足——断开状态下测速照常可用，参数不丢）
      serverRunning: false,
      clientTestRunning: false,
      clientError: null,
      serverError: null,
      records: s.records.map((r) =>
        r.status === "running"
          ? { ...r, status: "failed" as const, error: "session disconnected" }
          : r
      ),
    }),
    getStatus: loadBackendStatus,
  });

  // 客户端参数后端同步的防抖定时器与最近已同步值（防重复 invoke）
  const debounceRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const lastSyncedParamsRef = useRef("");
  // 卸载时清理未触发的防抖定时器
  useEffect(
    () => () => {
      if (debounceRef.current) clearTimeout(debounceRef.current);
    },
    []
  );

  // 从会话参数播种监听 IP/端口与版本。会话配置持久化（版本在连接对话框中
  // 配置）：重启加载的会话 params 含 version/listen_ip/listen_port。
  // 仅当 store 尚未加载时播种，不覆盖已加载状态（会话内 tab 切换不重置）
  // 依赖取原始值而非 tabs 数组身份：UPDATE_TAB_STATS 每秒替换数组引用，
  // 依赖数组身份会让 effect 每秒空跑（ConnectDialog 的 tabsRef 同款反模式）
  const seedTab = sessionState.tabs.find((t) => t.id === sessionId);
  const seedParams = (seedTab?.params ?? {}) as Record<string, unknown>;
  const seedVersion: IperfVersionStr =
    seedParams.version === "iperf3" ? "iperf3" : "iperf2";
  const seedListenIp =
    typeof seedParams.listen_ip === "string" ? seedParams.listen_ip : undefined;
  const seedListenPort =
    typeof seedParams.listen_port === "number"
      ? seedParams.listen_port
      : undefined;
  useEffect(() => {
    if (state.loaded) return;
    const listenPort =
      seedListenPort ?? (seedVersion === "iperf2" ? 5001 : 5201);
    if (!seedListenIp && listenPort === (seedVersion === "iperf2" ? 5001 : 5201)) {
      return; // 无有效播种参数（未持久化的会话）——保持默认
    }
    const s = api.getState();
    api.setState({
      version: seedVersion,
      listenIp: seedListenIp ?? s.listenIp,
      listenPort,
      // 客户端目标端口由后端 dynamic_params 恢复（loadBackendStatus），
      // 不在此以监听端口覆盖——用户配置的 -p 会被静默丢弃
    });
  }, [sessionId, state.loaded, seedVersion, seedListenIp, seedListenPort, api]);

  // 会话配置变更（右键"配置"/连接对话框编辑）同步到 store。
  // 已连接路径由 reconfigureSession 的断连→重连 → session-connected 事件驱动；
  // 未连接路径只有 tab.params 变化（reducer 以整体引用替换），此处对比应用，
  // 避免右侧面板停留旧值。首载由播种块负责（loaded=false 跳过，不与其竞争）。
  const sessionParams = sessionState.tabs.find((t) => t.id === sessionId)?.params;
  useEffect(() => {
    if (!state.loaded) return;
    const p = (sessionParams ?? {}) as Record<string, unknown>;
    const version: IperfVersionStr | undefined =
      p.version === "iperf2" || p.version === "iperf3" ? p.version : undefined;
    const listenIp = typeof p.listen_ip === "string" ? p.listen_ip : undefined;
    const listenPort =
      typeof p.listen_port === "number" ? p.listen_port : undefined;
    if (
      (version === undefined || version === state.version) &&
      (listenIp === undefined || listenIp === state.listenIp) &&
      (listenPort === undefined || listenPort === state.listenPort)
    ) {
      return; // 值未变化（如仅改名称）→ 不 setState，避免面板无谓刷新
    }
    api.setState({
      version: version ?? state.version,
      listenIp: listenIp ?? state.listenIp,
      listenPort: listenPort ?? state.listenPort,
      // 端口联动只作用于监听端口；客户端目标端口不随监听端口变化
    });
  }, [sessionParams, sessionId, state.loaded, state.version, state.listenIp, state.listenPort, state.clientParams, api]);

  // 客户端参数更新（表单 onChange → 本地 + 后端防抖同步）
  const handleClientParamsChange = useCallback(
    (params: IperfParams) => {
      api.setState({ clientParams: params });
      // 后端同步走 300ms trailing 防抖：逐键 invoke 会乱序应用并留下陈旧
      // 参数集；listenIp 一并回传（此前漏传 → 回退 "0.0.0.0" 污染监听配置）
      const s = api.getState();
      const backendParams = paramsToBackend(
        params,
        s.version,
        s.listenPort,
        s.listenIp
      );
      if (debounceRef.current) clearTimeout(debounceRef.current);
      debounceRef.current = setTimeout(() => {
        const key = JSON.stringify(backendParams);
        if (key === lastSyncedParamsRef.current) return;
        lastSyncedParamsRef.current = key;
        invoke("iperf_update_params", {
          sessionId,
          params: backendParams,
        }).catch((e) => console.error("[iperf] 参数同步失败:", e));
      }, 300);
    },
    [sessionId, api]
  );

  const handleTargetHostChange = useCallback(
    (host: string) => {
      api.setState({ targetHost: host });
    },
    [api]
  );

  // 运行客户端测速（invoke 立即返回；进度/结果由事件驱动）
  const handleClientRun = useCallback(async () => {
    const s = api.getState();
    if (!s.targetHost || s.clientTestRunning) return;
    // 清除上次错误 + 乐观占位运行标志：started 事件往返前双击/连点即被拦截
    // （后端守卫亦已同步置位，双重防线）
    api.setState({ clientError: null, clientTestRunning: true });
    const params = paramsToBackend(
      s.clientParams,
      s.version,
      s.listenPort,
      s.listenIp
    );
    try {
      await invoke("iperf_client_run", {
        sessionId,
        targetHost: s.targetHost,
        params,
      });
    } catch (e) {
      console.error("[iperf] 客户端测速失败:", e);
      // 错误可见：面板显示错误文本；记录兜底标记失败
      const cur = api.getState();
      const target = activeRecord(cur, "client", "fwd");
      api.setState({
        clientError: String(e),
        records: target
          ? cur.records.map((r) =>
              r.id === target.id
                ? { ...r, status: "failed" as const, error: String(e) }
                : r
            )
          : cur.records,
        clientTestRunning: false,
      });
    }
  }, [sessionId, api]);

  // 停止客户端测速：乐观更新（立即复位 UI），done 事件随后确认收尾
  const handleClientStop = useCallback(async () => {
    const s = api.getState();
    if (!s.clientTestRunning) return;
    api.setState({
      clientTestRunning: false,
      records: s.records.map((r) =>
        r.role === "client" && r.status === "running"
          ? { ...r, status: "failed" as const, error: "已停止" }
          : r
      ),
    });
    try {
      await invoke("iperf_client_stop", { sessionId });
    } catch (e) {
      console.error("[iperf] 停止失败:", e);
    }
  }, [sessionId, api]);

  const handleSelectRecord = useCallback(
    (id: string) => {
      api.setState({ selectedRecordId: id });
    },
    [api]
  );

  const selectedRecord =
    state.records.find((r) => r.id === state.selectedRecordId) ??
    state.records[0] ??
    null;

  return (
    <div className={styles.container}>
      {/* 顶部版本展示（版本在连接对话框中配置，会话内只读） */}
      <div className={`${styles.versionBar} liquid-glass-card`}>
        <span className={styles.versionLabel}>
          {t("iperf.version")}: {state.version}
        </span>
        <span className={styles.versionHint}>{t("iperf.versionHint")}</span>
      </div>

      {/* 左右分栏 */}
      <div className={styles.columns}>
        {/* 左：操作区 */}
        <div className={styles.leftCol}>
          <IperfServerPanel
            version={state.version}
            serverRunning={state.serverRunning}
            listenAddr={state.listenAddr}
            listenIp={state.listenIp}
            listenPort={state.listenPort}
            serverError={state.serverError}
          />
          <IperfClientPanel
            version={state.version}
            params={state.clientParams}
            targetHost={state.targetHost}
            testRunning={state.clientTestRunning}
            error={state.clientError}
            onParamsChange={handleClientParamsChange}
            onTargetHostChange={handleTargetHostChange}
            onRun={handleClientRun}
            onStop={handleClientStop}
          />
        </div>

        {/* 右：结果区 */}
        <div className={styles.rightCol}>
          <IperfRecordList
            records={state.records}
            selectedId={state.selectedRecordId ?? ""}
            onSelect={handleSelectRecord}
          />
          <IperfRecordDetail record={selectedRecord} />
        </div>
      </div>
    </div>
  );
}

// ═══════════════════════════════════════════════════════════════════
// 参数转换（前端 ↔ 后端）
// ═══════════════════════════════════════════════════════════════════

/** 前端参数 → 后端 IperfDynamicParams JSON */
export function paramsToBackend(
  p: IperfParams,
  version: IperfVersionStr,
  listenPort?: number,
  listenIp?: string
) {
  return {
    version,
    protocol: p.protocol,
    duration_secs: p.durationSecs,
    port: p.port,
    parallel_streams: p.parallelStreams,
    report_interval_secs: p.reportIntervalSecs,
    bandwidth_bps: p.bandwidthBps,
    bidirectional: p.bidirectional,
    tradeoff: p.tradeoff,
    window_size: p.windowSize,
    reverse: p.reverse,
    bidir: p.bidir,
    omit_secs: p.omitSecs,
    listen_ip: listenIp ?? "0.0.0.0",
    listen_port: listenPort ?? p.port,
  };
}
