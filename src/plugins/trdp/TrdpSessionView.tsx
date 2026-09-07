import { useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { open, save } from "@tauri-apps/plugin-dialog";
import { useTranslation } from "react-i18next";
import { useSession } from "../../context/SessionContext";
import Icon from "../../components/common/Icon";
import styles from "./TrdpSessionView.module.css";
import { TrdpAnalysisTables, TrdpPacketInspector } from "./TrdpPanels";

import {
  LIVE_CAPTURE_FRAME_LIMIT,
  STANDARD_CAPTURE_FILTER,
  captureFilterForPorts,
  createObject,
  defaultDatasetValues,
  draftsFromDecoded,
  draftsFromValues,
  isIpv4Text,
  isOneShotKind,
  missedBetween,
  paramNumber,
  workspaceDraftFromWorkspace,
  workspaceFromDraft,
  type CaptureFlowSummary,
  type CaptureInterface,
  type CaptureResult,
  type CaptureSummary,
  type DecodedDataset,
  type EncodedDataset,
  type FlowRow,
  type LinkChoice,
  type ObjectKind,
  type Page,
  type RedundancyState,
  type RuntimeState,
  type StructuredEditor,
  type TrdpEvent,
  type TrdpObject,
  type Workspace,
  type WorkspaceDraft,
  type XmlImport,
} from "./model";

const PACKET_PAGE_SIZE = 250;

function flowRowFromSummary(flow: CaptureFlowSummary): FlowRow {
  return {
    key: flow.key,
    msg: flow.msg,
    comId: flow.com_id,
    src: flow.src,
    dst: flow.dst,
    count: flow.count,
    lastSeq: flow.last_seq,
    size: flow.size,
    link: flow.link,
    missed: flow.missed,
    errors: flow.errors,
    minIntervalUs: flow.min_interval_us,
    avgIntervalUs: flow.avg_interval_us,
    maxIntervalUs: flow.max_interval_us,
    jitterUs: flow.jitter_us,
  };
}

const nav: Array<[Page, string]> = [
  ["overview", "trdp.nav.overview"],
  ["pd", "trdp.nav.pd"],
  ["md", "trdp.nav.md"],
  ["analysis", "trdp.nav.analysis"],
];

export default function TrdpSessionView({ sessionId }: { sessionId: string }) {
  const { t } = useTranslation();
  const { state } = useSession();
  const tab = state.tabs.find(item => item.id === sessionId);
  const params = tab?.params as Record<string, unknown> | undefined;
  const mode = (params?.mode as string | undefined) ?? "node";
  const sessionConnected = tab?.state === "connected" || tab?.state === "transferring";
  const configuredXmlPath = (
    typeof params?.xml_path === "string" && params.xml_path.trim()
      ? params.xml_path
      : null
  );
  const configuredWorkspace = (
    params?.trdp_workspace
    && typeof params.trdp_workspace === "object"
    && !Array.isArray(params.trdp_workspace)
  )
    ? params.trdp_workspace as Workspace
    : undefined;
  const [page, setPage] = useState<Page>(mode === "monitor" ? "analysis" : "overview");
  const [events, setEvents] = useState<TrdpEvent[]>([]);
  const [captureId, setCaptureId] = useState<string | null>(null);
  const captureIdRef = useRef<string | null>(null);
  const [captureSource, setCaptureSource] = useState<"offline" | "live" | null>(null);
  const [captureRunning, setCaptureRunning] = useState(false);
  const captureRunningRef = useRef(false);
  const [captureTransitioning, setCaptureTransitioning] = useState(false);
  const captureTransitioningRef = useRef(false);
  const viewMountedRef = useRef(true);

  function updateCaptureRunning(value: boolean) {
    captureRunningRef.current = value;
    setCaptureRunning(value);
  }

  function updateCaptureTransitioning(value: boolean) {
    captureTransitioningRef.current = value;
    setCaptureTransitioning(value);
  }
  const [captureFrameCount, setCaptureFrameCount] = useState(0);
  const [capturePacketCount, setCapturePacketCount] = useState(0);
  const [captureDroppedFrames, setCaptureDroppedFrames] = useState(0);
  const [captureFlows, setCaptureFlows] = useState<FlowRow[]>([]);
  const [packetPage, setPacketPage] = useState(0);
  const [pagedPackets, setPagedPackets] = useState<TrdpEvent[]>([]);
  const packetBatchRef = useRef<TrdpEvent[]>([]);
  const batchTimerRef = useRef<number | null>(null);
  const summaryTimerRef = useRef<number | null>(null);
  const [workspaceDraft, setWorkspaceDraft] = useState<WorkspaceDraft>(
    () => workspaceDraftFromWorkspace(configuredWorkspace),
  );
  const [workspaceLoaded, setWorkspaceLoaded] = useState(false);
  const objects = workspaceDraft.objects;
  const redundancyGroups = workspaceDraft.redundancyGroups;

  function setObjects(update: TrdpObject[] | ((previous: TrdpObject[]) => TrdpObject[])) {
    setWorkspaceDraft(previous => ({
      ...previous,
      objects: typeof update === "function" ? update(previous.objects) : update,
    }));
  }
  const [xmlImport, setXmlImport] = useState<XmlImport | null>(null);
  const [workspaceName, setWorkspaceName] = useState<string | null>(configuredWorkspace?.name ?? null);
  const [workspaceXmlPath, setWorkspaceXmlPath] = useState<string | null>(
    configuredWorkspace?.xml ?? null,
  );
  const [decoded, setDecoded] = useState<DecodedDataset | null>(null);
  const [selectedPacket, setSelectedPacket] = useState<TrdpEvent | null>(null);
  const mdRequestStartedUs = useRef(new Map<string, number>());
  const [structuredEditor, setStructuredEditor] = useState<StructuredEditor | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [selectedPdObjectId, setSelectedPdObjectId] = useState<string | null>(null);
  const [selectedMdObjectId, setSelectedMdObjectId] = useState<string | null>(null);
  const [pdFilter, setPdFilter] = useState<"all" | "publisher" | "subscriber" | "request">("all");
  const [mdFilter, setMdFilter] = useState<"all" | "request" | "listener" | "notify">("all");
  const [liveCaptureSetupOpen, setLiveCaptureSetupOpen] = useState(false);
  const [captureInterfaces, setCaptureInterfaces] = useState<CaptureInterface[]>([]);
  const [captureInterfacesLoading, setCaptureInterfacesLoading] = useState(false);
  const [captureInterfaceA, setCaptureInterfaceA] = useState(
    typeof params?.capture_interface === "string" ? params.capture_interface : "",
  );
  const [captureInterfaceBEnabled, setCaptureInterfaceBEnabled] = useState(
    params?.capture_interface_b_enabled === true,
  );
  const [captureInterfaceB, setCaptureInterfaceB] = useState(
    typeof params?.capture_interface_b === "string" ? params.capture_interface_b : "",
  );
  const initialCaptureFilter = typeof params?.capture_filter === "string"
    ? params.capture_filter
    : STANDARD_CAPTURE_FILTER;
  const [captureFilterAuto, setCaptureFilterAuto] = useState(
    typeof params?.capture_filter_auto === "boolean"
      ? params.capture_filter_auto
      : initialCaptureFilter === STANDARD_CAPTURE_FILTER,
  );
  const [captureFilter, setCaptureFilter] = useState(initialCaptureFilter);

  useEffect(() => {
    let cancelled = false;
    setWorkspaceLoaded(false);
    void invoke<Workspace | null>("trdp_command", {
      sessionId,
      command: { command: "workspace_get" },
    }).then(workspace => {
      if (cancelled) return;
      if (workspace?.format === "tauterm-trdp-workspace/v2") {
        setWorkspaceDraft(workspaceDraftFromWorkspace(workspace));
        setWorkspaceName(workspace.name ?? null);
        if (mode === "node") {
          void invoke<RuntimeState>("trdp_command", {
            sessionId,
            command: { command: "runtime_state" },
          }).then(runtime => {
            if (cancelled) return;
            setWorkspaceDraft(previous => ({
              ...previous,
              objects: previous.objects.map(object => ({
                ...object,
                state: runtime.objects[object.id] ?? "stopped",
              })),
            }));
          }).catch(() => {
            // A disconnected session has no sidechannel; persisted objects stay stopped.
          });
        }
        setWorkspaceXmlPath(workspace.xml ?? null);
        setXmlImport(null);
        setDecoded(null);
        if (workspace.xml) {
          void invoke<XmlImport>("trdp_command", {
            sessionId,
            command: { command: "xml_import", path: workspace.xml },
          }).then(imported => {
            if (!cancelled) {
              setXmlImport(imported);
              setWorkspaceXmlPath(imported.path);
            }
          }).catch(cause => {
            if (!cancelled) console.warn("TRDP Workspace XML 恢复失败:", cause);
          });
        }
      } else {
        setWorkspaceDraft(workspaceDraftFromWorkspace(null));
        setWorkspaceName(null);
        setWorkspaceXmlPath(configuredXmlPath);
        setXmlImport(null);
        setDecoded(null);
        if (configuredXmlPath) {
          void invoke<XmlImport>("trdp_command", {
            sessionId,
            command: { command: "xml_import", path: configuredXmlPath },
          }).then(imported => {
            if (!cancelled) {
              setXmlImport(imported);
              setWorkspaceXmlPath(imported.path);
            }
          }).catch(cause => {
            if (!cancelled) console.warn("TRDP 配置 XML 自动导入失败:", cause);
          });
        }
      }
      setWorkspaceLoaded(true);
    }).catch(cause => {
      if (!cancelled) {
        console.warn("TRDP Workspace 恢复失败:", cause);
        setWorkspaceLoaded(true);
      }
    });
    return () => { cancelled = true; };
  }, [sessionId, mode, configuredXmlPath]);

  useEffect(() => {
    if (!workspaceLoaded) return;
    const timer = window.setTimeout(() => {
      const workspace = workspaceFromDraft(
        workspaceDraft,
        workspaceName ?? undefined,
        xmlImport?.path ?? workspaceXmlPath ?? undefined,
      );
      void invoke("trdp_command", {
        sessionId,
        command: { command: "workspace_store", workspace },
      }).catch(cause => {
        console.warn("TRDP Workspace 持久化失败:", cause);
      });
    }, 250);
    return () => window.clearTimeout(timer);
  }, [
    sessionId,
    workspaceLoaded,
    workspaceDraft,
    workspaceName,
    workspaceXmlPath,
    xmlImport?.path,
  ]);

  useEffect(() => {
    if (sessionConnected) return;
    updateCaptureRunning(false);
    updateCaptureTransitioning(false);
    setWorkspaceDraft(previous => ({
      ...previous,
      objects: previous.objects.map(object => (
        object.state === "stopped" ? object : { ...object, state: "stopped" }
      )),
    }));
  }, [sessionConnected]);

  useEffect(() => {
    viewMountedRef.current = true;
    return () => {
      viewMountedRef.current = false;
      if (captureRunningRef.current || captureTransitioningRef.current) {
        void invoke("trdp_command", {
          sessionId,
          command: { command: "capture_stop" },
        });
      }
      const current = captureIdRef.current;
      if (current) void invoke("trdp_release_capture", { captureId: current });
    };
  }, [sessionId]);


  useEffect(() => {
    let disposed = false;

    const flushBatches = () => {
      batchTimerRef.current = null;
      const packets = packetBatchRef.current.splice(0);
      if (packets.length > 0) {
        setEvents(prev => [...prev, ...packets].slice(-5000));
      }
    };

    const scheduleFlush = () => {
      if (batchTimerRef.current !== null) return;
      batchTimerRef.current = window.setTimeout(flushBatches, 40);
    };

    const unlisten = listen<TrdpEvent>("trdp-event", ({ payload }) => {
      if (disposed || payload.session_id !== sessionId) return;
      if (payload.event === "md_session" && payload.md_session_id && payload.timestamp_us !== undefined) {
        const starts = mdRequestStartedUs.current;
        starts.set(payload.md_session_id, payload.timestamp_us);
        if (starts.size > 1024) {
          const oldest = starts.keys().next().value;
          if (typeof oldest === "string") starts.delete(oldest);
        }
      }
      if (payload.event === "capture_progress") {
        setCaptureFrameCount(payload.frame_count ?? 0);
        setCapturePacketCount(payload.packet_count ?? 0);
        setCaptureDroppedFrames(payload.dropped_frames ?? 0);
        if (payload.capture_id && summaryTimerRef.current === null) {
          const summaryCaptureId = payload.capture_id;
          summaryTimerRef.current = window.setTimeout(() => {
            summaryTimerRef.current = null;
            void invoke<CaptureSummary>("trdp_capture_summary", {
              captureId: summaryCaptureId,
            }).then(summary => {
              if (disposed || captureIdRef.current !== summaryCaptureId) return;
              setCapturePacketCount(summary.packet_count);
              setCaptureFlows(summary.flows.map(flowRowFromSummary));
            }).catch(() => {
              // Capture may have been released while the throttled refresh was pending.
            });
          }, 250);
        }
      }
      if (payload.event === "object_state" && payload.id && payload.state) {
        setWorkspaceDraft(previous => ({
          ...previous,
          objects: previous.objects.map(object => (
            object.id === payload.id ? { ...object, state: payload.state! } : object
          )),
        }));
      }
      if (payload.event === "packet") {
        let packet = payload;
        if (
          payload.md_session_id
          && payload.timestamp_us !== undefined
          && ["Mp", "Mq", "Me"].includes(payload.msg_type ?? "")
        ) {
          const started = mdRequestStartedUs.current.get(payload.md_session_id);
          if (started !== undefined && payload.timestamp_us >= started) {
            packet = { ...payload, latency_us: payload.timestamp_us - started };
          }
        }
        packetBatchRef.current.push(packet);
        scheduleFlush();
        if (payload.about_to_die && payload.md_session_id) {
          mdRequestStartedUs.current.delete(payload.md_session_id);
        }
      }
      if (payload.error) setError(payload.error);
    });

    return () => {
      disposed = true;
      if (batchTimerRef.current !== null) {
        window.clearTimeout(batchTimerRef.current);
        batchTimerRef.current = null;
      }
      packetBatchRef.current = [];
      if (summaryTimerRef.current !== null) {
        window.clearTimeout(summaryTimerRef.current);
        summaryTimerRef.current = null;
      }
      void unlisten.then(fn => fn());
    };
  }, [sessionId]);

  const datasetByComId = useMemo(() => {
    const result = new Map<number, number>();
    for (const telegram of xmlImport?.telegrams ?? []) result.set(telegram.com_id, telegram.dataset_id);
    return result;
  }, [xmlImport]);

  const expectedCycleByComId = useMemo(() => {
    const result = new Map<number, number>();
    for (const object of objects) {
      if (object.kind === "pd_publisher") result.set(object.comId, object.cycleUs);
    }
    for (const telegram of xmlImport?.telegrams ?? []) {
      if (telegram.cycle_us && !result.has(telegram.com_id)) result.set(telegram.com_id, telegram.cycle_us);
    }
    return result;
  }, [objects, xmlImport]);

  const eventFlows = useMemo(() => {
    type MutableFlow = FlowRow & { intervals: number[]; previousSeq?: number; previousTimestamp?: number };
    const map = new Map<string, MutableFlow>();
    for (const event of events) {
      if (event.com_id === undefined) continue;
      const key = `${event.link ?? ""}:${event.msg_type ?? event.kind}:${event.com_id}:${event.src_ip ?? ""}:${event.dest_ip ?? ""}`;
      const row = map.get(key) ?? {
        key,
        msg: event.msg_type ?? event.kind ?? "?",
        comId: event.com_id,
        src: event.src_ip ?? "—",
        dst: event.dest_ip ?? "—",
        count: 0,
        link: event.link ?? "—",
        missed: 0,
        errors: 0,
        intervals: [],
      };
      row.count += 1;
      if (
        (event.result_code !== undefined && event.result_code !== 0)
        || event.crc_valid === false
        || event.protocol_valid === false
      ) {
        row.errors += 1;
      }
      if (event.seq_count !== undefined) {
        if (row.previousSeq !== undefined) row.missed += missedBetween(row.previousSeq, event.seq_count);
        row.previousSeq = event.seq_count;
        row.lastSeq = event.seq_count;
      }
      if (event.timestamp_us !== undefined) {
        if (row.previousTimestamp !== undefined && event.timestamp_us >= row.previousTimestamp) {
          row.intervals.push(event.timestamp_us - row.previousTimestamp);
        }
        row.previousTimestamp = event.timestamp_us;
      }
      row.size = event.data_len;
      map.set(key, row);
    }
    return [...map.values()].map(row => {
      const expected = expectedCycleByComId.get(row.comId);
      if (row.intervals.length) {
        row.minIntervalUs = Math.min(...row.intervals);
        row.maxIntervalUs = Math.max(...row.intervals);
        row.avgIntervalUs = row.intervals.reduce((sum, value) => sum + value, 0) / row.intervals.length;
        if (expected !== undefined) row.jitterUs = row.intervals.reduce((sum, value) => sum + Math.abs(value - expected), 0) / row.intervals.length;
      }
      const { intervals: _intervals, previousSeq: _previousSeq, previousTimestamp: _previousTimestamp, ...result } = row;
      return result;
    });
  }, [events, expectedCycleByComId]);
  const flows = captureId ? captureFlows : eventFlows;

  async function command<T = unknown>(name: string, payload: Record<string, unknown> = {}): Promise<T> {
    setError(null);
    try {
      return await invoke<T>("trdp_command", { sessionId, command: { command: name, ...payload } });
    } catch (cause) {
      setError(String(cause));
      throw cause;
    }
  }

  function requireRuntimeConnection() {
    if (sessionConnected) return true;
    setError(t("trdp.errors.connectRequired"));
    return false;
  }

  async function loadOfflinePacketPage(
    nextPage: number,
    id = captureIdRef.current,
    total = capturePacketCount,
  ) {
    if (!id || total <= 0) {
      setPacketPage(0);
      setPagedPackets([]);
      return;
    }
    const pageCount = Math.max(1, Math.ceil(total / PACKET_PAGE_SIZE));
    const page = Math.max(0, Math.min(nextPage, pageCount - 1));
    const remaining = Math.max(0, total - page * PACKET_PAGE_SIZE);
    const count = Math.min(PACKET_PAGE_SIZE, remaining);
    const offset = Math.max(0, remaining - count);
    const packets = await invoke<TrdpEvent[]>("trdp_capture_packets", {
      captureId: id,
      offset,
      limit: count,
    });
    setPacketPage(page);
    setPagedPackets(packets.slice().reverse());
    setSelectedPacket(null);
    setDecoded(null);
  }

  function adoptCapture(nextCaptureId: string | null) {
    const previous = captureIdRef.current;
    captureIdRef.current = nextCaptureId;
    setCaptureId(nextCaptureId);
    if (previous && previous !== nextCaptureId) {
      void invoke("trdp_release_capture", { captureId: previous });
    }
  }

  function clearCaptureView() {
    if (captureTransitioning) return;
    adoptCapture(null);
    setCaptureSource(null);
    updateCaptureRunning(false);
    setCaptureFrameCount(0);
    setCapturePacketCount(0);
    setCaptureDroppedFrames(0);
    setCaptureFlows([]);
    setPacketPage(0);
    setPagedPackets([]);
    setEvents([]);
    setSelectedPacket(null);
    setDecoded(null);
  }


  function addObject(kind: ObjectKind) {
    const item = createObject(kind, objects.filter(candidate => candidate.kind === kind).length + 1);
    setObjects(prev => [...prev, item]);
    if (kind.startsWith("pd_")) {
      setSelectedPdObjectId(item.id);
      setPage("pd");
    } else {
      setSelectedMdObjectId(item.id);
      setPage("md");
    }
  }

  function patchObject(id: string, patch: Partial<TrdpObject>) {
    setWorkspaceDraft(previous => {
      const redundancyGroups = { ...previous.redundancyGroups };
      if (patch.redId !== undefined && patch.redId > 0) {
        const key = String(patch.redId);
        if (!redundancyGroups[key]) redundancyGroups[key] = "leader";
      }
      return {
        ...previous,
        objects: previous.objects.map(item => item.id === id ? { ...item, ...patch } : item),
        redundancyGroups,
      };
    });
  }

  async function startObject(obj: TrdpObject) {
    if (!requireRuntimeConnection()) return;
    const oneShot = isOneShotKind(obj.kind);
    patchObject(obj.id, { state: oneShot ? "sending" : "starting" });
    try {
      if (obj.kind === "pd_request") {
        // A PD Request retains a native subscriber during its reply window.
        // Explicitly replace that handle before a repeated Send.
        await command("object_stop", { id: obj.id, kind: obj.kind });
      }
      await command("object_start", {
        object: {
          id: obj.id,
          kind: obj.kind,
          name: obj.name,
          com_id: obj.comId,
          link: obj.link,
          destination: obj.destination,
          source: obj.source,
          cycle_us: obj.cycleUs,
          timeout_us: obj.timeoutMode === "disabled" ? 0xffff_ffff : obj.timeoutMode === "custom" ? obj.timeoutUs : 0,
          timeout_behavior: obj.timeoutBehavior,
          payload_hex: obj.payloadHex,
          transport: obj.transport,
          etb_topo_count: obj.etbTopoCount,
          op_trn_topo_count: obj.opTrnTopoCount,
          red_id: obj.redId,
          red_state: obj.redId > 0 ? (redundancyGroups[String(obj.redId)] ?? "leader") : "leader",
          num_replies: obj.numReplies,
          reply_timeout_us: obj.replyTimeoutUs,
          response_mode: obj.responseMode,
          confirm_timeout_us: obj.confirmTimeoutUs,
          reply_com_id: obj.replyComId,
          reply_ip: obj.replyIp,
          source_uri: obj.sourceUri,
          dest_uri: obj.destUri,
        },
      });
      patchObject(obj.id, { state: oneShot ? "stopped" : "running" });
    } catch {
      patchObject(obj.id, { state: "error" });
    }
  }

  async function stopObject(obj: TrdpObject) {
    if (!requireRuntimeConnection()) return;
    patchObject(obj.id, { state: "stopping" });
    try {
      await command("object_stop", { id: obj.id, kind: obj.kind });
      patchObject(obj.id, { state: "stopped" });
    } catch {
      patchObject(obj.id, { state: "error" });
    }
  }

  async function removeObject(obj: TrdpObject) {
    if (obj.state !== "stopped" && obj.state !== "error") return;
    // PD Request is one-shot in the UI but TCNOpen retains a subscriber handle
    // for its reply window. Clean that native handle only while this Node
    // session is actually connected. Once disconnected, the side-channel and
    // all native handles are already gone; sending object_stop would fail and
    // incorrectly block deletion of the local object.
    if (obj.kind === "pd_request" && sessionConnected) {
      await command("object_stop", { id: obj.id, kind: obj.kind });
    }
    setObjects(prev => prev.filter(item => item.id !== obj.id));
  }

  async function updateRedundancyGroup(redId: number, nextState: RedundancyState) {
    if (redId <= 0) return;
    const key = String(redId);
    const previousState = redundancyGroups[key] ?? "leader";
    setWorkspaceDraft(previous => ({
      ...previous,
      redundancyGroups: { ...previous.redundancyGroups, [key]: nextState },
    }));
    const hasRunningPublisher = objects.some(
      item => item.kind === "pd_publisher" && item.redId === redId && item.state === "running",
    );
    if (tab?.state === "connected" && hasRunningPublisher) {
      try {
        await command("redundancy_set", { red_id: redId, red_state: nextState, link: "both" });
      } catch {
        setWorkspaceDraft(previous => ({
          ...previous,
          redundancyGroups: { ...previous.redundancyGroups, [key]: previousState },
        }));
      }
    }
  }

  async function updatePayload(obj: TrdpObject) {
    if (!requireRuntimeConnection()) return;
    await command("object_update", { id: obj.id, payload_hex: obj.payloadHex });
  }

  async function openCapture() {
    if (captureRunning || captureTransitioning) return;
    const path = await open({ multiple: false, filters: [{ name: "Packet Capture", extensions: ["pcap", "pcapng"] }] });
    if (typeof path !== "string") return;
    const pdPort = paramNumber(params, "pd_port", 17224);
    const mdPorts = [...new Set([paramNumber(params, "md_udp_port", 17225), paramNumber(params, "md_tcp_port", 17225)])];
    const expectedCycles = Object.fromEntries(expectedCycleByComId);
    const result = await invoke<CaptureResult>("trdp_open_capture", {
      path,
      pdPorts: [pdPort],
      mdPorts,
      expectedCycles,
    });
    adoptCapture(result.capture_id);
    setCaptureSource("offline");
    updateCaptureRunning(false);
    setCaptureFrameCount(result.frame_count);
    setCapturePacketCount(result.packet_count);
    setCaptureDroppedFrames(result.dropped_frames);
    setCaptureFlows(result.flows.map(flowRowFromSummary));
    setEvents(result.packets.slice(-5000));
    setPacketPage(0);
    setPagedPackets(result.packets.slice(-PACKET_PAGE_SIZE).reverse());
    setSelectedPacket(null);
    setDecoded(null);
    setPage("analysis");
    await loadOfflinePacketPage(0, result.capture_id, result.packet_count);
  }

  async function saveCapture() {
    if (!captureId) return;
    const path = await save({ filters: [{ name: "PCAPNG", extensions: ["pcapng"] }] });
    if (!path) return;
    await invoke("trdp_save_capture", { path, captureId });
  }

  async function importXml() {
    const configured = typeof params?.xml_path === "string" ? params.xml_path : "";
    let path = configured;
    if (!path) {
      const selected = await open({ multiple: false, filters: [{ name: "TRDP XML", extensions: ["xml"] }] });
      if (typeof selected !== "string") return;
      path = selected;
    }
    const imported = await command<XmlImport>("xml_import", { path });
    setXmlImport(imported);
    setWorkspaceXmlPath(imported.path);
    setDecoded(null);
  }

  async function importWorkspace() {
    const selected = await open({ multiple: false, filters: [{ name: "TauTerm TRDP Workspace", extensions: ["json"] }] });
    if (typeof selected !== "string") return;
    const workspace = await command<Workspace>("workspace_import", { path: selected });
    setWorkspaceDraft(workspaceDraftFromWorkspace(workspace));
    setWorkspaceName(workspace.name ?? selected);
    setXmlImport(null);
    setDecoded(null);
    const importedXmlPath = workspace.xml_path ?? workspace.xml ?? null;
    setWorkspaceXmlPath(importedXmlPath);
    if (importedXmlPath) {
      const importedXml = await command<XmlImport>("xml_import", { path: importedXmlPath });
      setXmlImport(importedXml);
      setWorkspaceXmlPath(importedXml.path);
    }
  }

  async function inspectPacket(event: TrdpEvent) {
    setSelectedPacket(event);
    setDecoded(null);
    const datasetId = event.com_id === undefined ? undefined : datasetByComId.get(event.com_id);
    if (!xmlImport || !datasetId || !event.payload_hex) return;
    try {
      const result = await command<DecodedDataset>("dataset_decode", {
        path: xmlImport.path,
        dataset_id: datasetId,
        payload_hex: event.payload_hex,
      });
      setDecoded(result);
    } catch { /* error banner already populated */ }
  }

  async function openStructuredEditor(obj: TrdpObject) {
    if (!xmlImport) {
      setError("先导入 TRDP XML 才能使用 Dataset structured editor。");
      return;
    }
    const datasetId = datasetByComId.get(obj.comId);
    if (!datasetId) {
      setError(`ComID ${obj.comId} 没有 XML Dataset 映射。`);
      return;
    }
    if (obj.payloadHex) {
      try {
        const result = await command<DecodedDataset>("dataset_decode", {
          path: xmlImport.path,
          dataset_id: datasetId,
          payload_hex: obj.payloadHex,
        });
        setStructuredEditor({ objectId: obj.id, datasetId, drafts: draftsFromDecoded(result) });
        return;
      } catch {
        return;
      }
    }
    setStructuredEditor({
      objectId: obj.id,
      datasetId,
      drafts: draftsFromValues(defaultDatasetValues(xmlImport, datasetId)),
    });
  }

  async function decodeStructuredFromHex() {
    if (!structuredEditor || !xmlImport) return;
    const obj = objects.find(item => item.id === structuredEditor.objectId);
    if (!obj || !obj.payloadHex) {
      setError("当前对象没有可解码的 Payload HEX。");
      return;
    }
    try {
      const result = await command<DecodedDataset>("dataset_decode", {
        path: xmlImport.path,
        dataset_id: structuredEditor.datasetId,
        payload_hex: obj.payloadHex,
      });
      setStructuredEditor(prev => prev ? { ...prev, drafts: draftsFromDecoded(result) } : prev);
    } catch {
      // command() already populated the error banner.
    }
  }

  async function applyStructuredToHex() {
    if (!structuredEditor || !xmlImport) return;
    const obj = objects.find(item => item.id === structuredEditor.objectId);
    if (!obj) return;
    const values: Record<string, unknown> = {};
    try {
      for (const [name, draft] of Object.entries(structuredEditor.drafts)) {
        values[name] = JSON.parse(draft);
      }
    } catch (cause) {
      setError(`Structured field 必须是合法 JSON 值: ${String(cause)}`);
      return;
    }
    try {
      const encoded = await command<EncodedDataset>("dataset_encode", {
        path: xmlImport.path,
        dataset_id: structuredEditor.datasetId,
        values,
      });
      patchObject(obj.id, { payloadHex: encoded.payload_hex });
      if (
        sessionConnected
        && obj.state === "running"
        && (obj.kind === "pd_publisher" || obj.kind === "md_listener")
      ) {
        await command("object_update", { id: obj.id, payload_hex: encoded.payload_hex });
      }
    } catch {
      // command() already populated the error banner.
    }
  }

  function importTemplates() {
    if (!xmlImport) return;
    setObjects(prev => {
      const known = new Set(prev.map(item => `${item.comId}:${item.destination}`));
      const additions: TrdpObject[] = [];
      for (const telegram of xmlImport.telegrams) {
        if (telegram.traffic_kind !== "pd") continue;
        const ipv4Destinations = telegram.destinations.filter(isIpv4Text);
        const templateDestinations = ipv4Destinations.length > 0 ? ipv4Destinations : ["0.0.0.0"];
        const source = telegram.sources.find(isIpv4Text) ?? "0.0.0.0";
        for (const destination of templateDestinations) {
          if (known.has(`${telegram.com_id}:${destination}`)) continue;
          const item = createObject("pd_subscriber", additions.length + 1);
          item.name = `${telegram.name} (imported template)`;
          item.comId = telegram.com_id;
          item.destination = destination;
          item.source = source;
          if (telegram.timeout_us === undefined || telegram.timeout_us === 0) {
            item.timeoutMode = "disabled";
          } else {
            item.timeoutMode = "custom";
            item.timeoutUs = telegram.timeout_us;
          }
          item.timeoutBehavior = telegram.timeout_behavior === "keep" ? "keep" : "zero";
          additions.push(item);
          known.add(`${telegram.com_id}:${destination}`);
        }
      }
      return [...prev, ...additions];
    });
  }

  async function refreshCaptureInterfaces() {
    setCaptureInterfacesLoading(true);
    setError(null);
    try {
      const items = await invoke<CaptureInterface[]>("trdp_capture_interfaces");
      setCaptureInterfaces(items);
      setCaptureInterfaceA(current => current || items[0]?.name || "");
      setCaptureInterfaceB(current => {
        if (current && current !== (captureInterfaceA || items[0]?.name || "")) return current;
        return items.find(item => item.name !== (captureInterfaceA || items[0]?.name || ""))?.name || "";
      });
      if (items.length === 0) setError(t("trdp.captureInterfaces.empty"));
    } catch (cause) {
      setCaptureInterfaces([]);
      setError(`${t("trdp.captureInterfaces.error")}: ${String(cause)}`);
    } finally {
      setCaptureInterfacesLoading(false);
    }
  }

  async function toggleLiveCaptureSetup() {
    if (liveCaptureSetupOpen) {
      setLiveCaptureSetupOpen(false);
      return;
    }
    setLiveCaptureSetupOpen(true);
    if (captureInterfaces.length === 0 && !captureInterfacesLoading) {
      await refreshCaptureInterfaces();
    }
  }

  async function startLiveCapture() {
    if (captureTransitioning || captureRunning) return;
    if (!requireRuntimeConnection()) return;
    if (!captureInterfaceA) {
      setError(t("trdp.captureInterfaces.choose"));
      return;
    }
    const interfaceA = captureInterfaceA;
    const interfaceB = captureInterfaceBEnabled ? captureInterfaceB : "";
    const filter = captureFilterAuto
      ? captureFilterForPorts(
          paramNumber(params, "pd_port", 17224),
          paramNumber(params, "md_udp_port", 17225),
          paramNumber(params, "md_tcp_port", 17225),
        )
      : captureFilter;
    const previous = {
      captureId,
      source: captureSource,
      running: captureRunning,
      frameCount: captureFrameCount,
      packetCount: capturePacketCount,
      droppedFrames: captureDroppedFrames,
      flows: captureFlows,
      packetPage,
      pagedPackets,
      events,
    };
    updateCaptureTransitioning(true);
    updateCaptureRunning(false);
    setCaptureFrameCount(0);
    setCapturePacketCount(0);
    setCaptureDroppedFrames(0);
    setCaptureFlows([]);
    setPacketPage(0);
    setPagedPackets([]);
    setEvents([]);
    setSelectedPacket(null);
    setDecoded(null);
    try {
      const result = await command<{ capture_id: string }>("capture_start", {
        interface: interfaceA,
        interface_b: interfaceB,
        filter,
        expected_cycles: Object.fromEntries(expectedCycleByComId),
      });
      if (!viewMountedRef.current) {
        void invoke("trdp_release_capture", { captureId: result.capture_id });
        return;
      }
      adoptCapture(result.capture_id);
      setCaptureSource("live");
      updateCaptureRunning(true);
    } catch {
      captureIdRef.current = previous.captureId;
      setCaptureId(previous.captureId);
      setCaptureSource(previous.source);
      updateCaptureRunning(previous.running);
      setCaptureFrameCount(previous.frameCount);
      setCapturePacketCount(previous.packetCount);
      setCaptureDroppedFrames(previous.droppedFrames);
      setCaptureFlows(previous.flows);
      setPacketPage(previous.packetPage);
      setPagedPackets(previous.pagedPackets);
      setEvents(previous.events);
    } finally {
      updateCaptureTransitioning(false);
    }
  }

  async function stopLiveCapture() {
    if (captureTransitioning || !captureRunning) return;
    if (!requireRuntimeConnection()) return;
    updateCaptureTransitioning(true);
    try {
      await command("capture_stop");
      updateCaptureRunning(false);
      const currentCaptureId = captureIdRef.current;
      if (currentCaptureId) {
        const summary = await invoke<CaptureSummary>("trdp_capture_summary", {
          captureId: currentCaptureId,
        });
        if (captureIdRef.current === currentCaptureId) {
          setCapturePacketCount(summary.packet_count);
          setCaptureFlows(summary.flows.map(flowRowFromSummary));
        }
      }
    } catch {
      // command() owns the error banner; keep the current state unchanged.
    } finally {
      updateCaptureTransitioning(false);
    }
  }

  async function confirmMessage(event: TrdpEvent) {
    if (!event.md_session_id || event.can_confirm !== true || !requireRuntimeConnection()) return;
    await command("md_confirm", {
      md_session_id: event.md_session_id,
      link: event.link ?? "a",
      user_status: 0,
    });
    setSelectedPacket(current => (
      current?.md_session_id === event.md_session_id
        ? { ...current, can_confirm: false }
        : current
    ));
    setEvents(previous => previous.map(candidate => (
      candidate.md_session_id === event.md_session_id && candidate.msg_type === "Mq"
        ? { ...candidate, can_confirm: false }
        : candidate
    )));
  }

  function mdLatencyUs(event: TrdpEvent) {
    if (event.latency_us !== undefined) return event.latency_us;
    if (!event.md_session_id || event.timestamp_us === undefined || !["Mp", "Mq", "Me"].includes(event.msg_type ?? "")) {
      return undefined;
    }
    const capturedRequests = events.filter(candidate =>
      candidate.md_session_id === event.md_session_id
      && candidate.msg_type === "Mr"
      && candidate.timestamp_us !== undefined
      && candidate.timestamp_us <= event.timestamp_us!,
    );
    const capturedRequest = capturedRequests.length > 0
      ? capturedRequests[capturedRequests.length - 1]
      : undefined;
    const started = capturedRequest?.timestamp_us ?? mdRequestStartedUs.current.get(event.md_session_id);
    return started === undefined || event.timestamp_us < started ? undefined : event.timestamp_us - started;
  }

  function observedMdReplies(event: TrdpEvent) {
    if (!event.md_session_id) return undefined;
    return events.filter(candidate =>
      candidate.md_session_id === event.md_session_id
      && ["Mp", "Mq"].includes(candidate.msg_type ?? ""),
    ).length;
  }

  const structuredObject = structuredEditor ? objects.find(item => item.id === structuredEditor.objectId) : undefined;
  const structuredDataset = structuredEditor && xmlImport
    ? xmlImport.datasets.find(item => item.id === structuredEditor.datasetId)
    : undefined;

  function objectKindLabel(kind: ObjectKind) {
    return t(`trdp.objectKind.${kind}`);
  }

  function objectStateLabel(state: TrdpObject["state"]) {
    return t(`trdp.status.${state}`);
  }

  function objectSummaryTable(items: TrdpObject[], selectedId: string | null, onSelect: (id: string) => void) {
    return (
      <div className={styles.tableWrap}>
        <table className={`${styles.table} ${styles.objectListTable}`}>
          <thead>
            <tr><th>{t("trdp.table.name")}</th><th>{t("trdp.table.type")}</th><th>ComID</th><th>{t("trdp.table.link")}</th><th>{t("trdp.table.destination")}</th><th>{t("trdp.table.state")}</th></tr>
          </thead>
          <tbody>
            {items.length === 0 ? (
              <tr><td colSpan={6} className={styles.emptyState}>{t("trdp.empty.noObjects")}</td></tr>
            ) : items.map(obj => (
              <tr
                key={obj.id}
                className={selectedId === obj.id ? styles.selectedRow : ""}
                onClick={() => onSelect(obj.id)}
              >
                <td>{obj.name}</td>
                <td>{objectKindLabel(obj.kind)}</td>
                <td>{obj.comId}</td>
                <td>{obj.link === "both" ? "A+B" : obj.link.toUpperCase()}</td>
                <td>{obj.destination}</td>
                <td>{objectStateLabel(obj.state)}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    );
  }

  function renderObjectDetail(obj: TrdpObject | undefined) {
    if (!obj) return <div className={`${styles.infoCard} liquid-glass-card`}>{t("trdp.empty.selectObject")}</div>;
    const oneShot = isOneShotKind(obj.kind);
    const busy = obj.state === "starting" || obj.state === "stopping" || obj.state === "sending";
    const subscriber = obj.kind === "pd_subscriber" || obj.kind === "pd_request";
    const redundancyState = obj.redId > 0 ? (redundancyGroups[String(obj.redId)] ?? "leader") : "leader";
    const inputClass = `${styles.detailInput} liquid-glass-input`;
    const selectClass = `${styles.detailInput} liquid-glass-input liquid-glass-select`;
    return (
      <div className={`${styles.objectDetail} liquid-glass-card`}>
        <div className={styles.objectDetailHeader}>
          <div>
            <strong>{obj.name}</strong>
            <span>{objectKindLabel(obj.kind)} · ComID {obj.comId}</span>
          </div>
          <div className={styles.rowActions}>
            {oneShot ? (
              <button className={`${styles.compactButton} liquid-primary-button`} onClick={() => void startObject(obj)} disabled={busy || !sessionConnected}>{t("trdp.actions.send")}</button>
            ) : (
              <button className={`${styles.compactButton} ${obj.state === "running" ? "liquid-glass-button" : "liquid-primary-button"}`} onClick={() => void (obj.state === "running" ? stopObject(obj) : startObject(obj))} disabled={busy || !sessionConnected}>
                {obj.state === "running" ? t("trdp.actions.stop") : t("trdp.actions.start")}
              </button>
            )}
            {obj.state === "running" && (obj.kind === "pd_publisher" || obj.kind === "md_listener") && (
              <button className={`${styles.compactButton} liquid-glass-button`} onClick={() => void updatePayload(obj)} disabled={!sessionConnected}>{t("trdp.actions.update")}</button>
            )}
            {obj.kind !== "pd_subscriber" && datasetByComId.has(obj.comId) && (
              <button className={`${styles.compactButton} liquid-glass-button`} onClick={() => void openStructuredEditor(obj)}>{t("trdp.actions.dataset")}</button>
            )}
            <button className={`${styles.compactButton} liquid-glass-button`} onClick={() => void removeObject(obj)} disabled={obj.state !== "stopped" && obj.state !== "error"}>{t("trdp.actions.remove")}</button>
          </div>
        </div>

        <div className={styles.objectDetailGrid}>
          <label><span>{t("trdp.table.name")}</span><input className={inputClass} value={obj.name} onChange={event => patchObject(obj.id, { name: event.target.value })} /></label>
          <label><span>ComID</span><input className={inputClass} type="number" min={1} value={obj.comId} onChange={event => patchObject(obj.id, { comId: Number(event.target.value) })} /></label>
          <label><span>{t("trdp.table.link")}</span><select className={selectClass} value={obj.link} onChange={event => patchObject(obj.id, { link: event.target.value as LinkChoice })}><option value="a">A</option><option value="b">B</option><option value="both">A+B</option></select></label>
          <label><span>{t("trdp.table.destination")}</span><input className={inputClass} value={obj.destination} onChange={event => patchObject(obj.id, { destination: event.target.value })} /></label>
          {obj.kind.startsWith("md_") ? (
            <label><span>{t("trdp.table.udpTcp")}</span><select className={selectClass} value={obj.transport} onChange={event => patchObject(obj.id, { transport: event.target.value as "udp" | "tcp" })}><option value="udp">UDP</option><option value="tcp">TCP</option></select></label>
          ) : subscriber ? (
            <label><span>{t("trdp.table.timeout")}</span><select className={selectClass} value={obj.timeoutMode} onChange={event => patchObject(obj.id, { timeoutMode: event.target.value as "auto" | "custom" | "disabled" })}><option value="auto">{t("trdp.status.auto")}</option><option value="custom">{t("trdp.status.custom")}</option><option value="disabled">{t("trdp.status.disabled")}</option></select></label>
          ) : (
            <label><span>{t("trdp.table.cycleUs")}</span><input className={inputClass} type="number" min={1} value={obj.cycleUs} onChange={event => patchObject(obj.id, { cycleUs: Number(event.target.value) })} /></label>
          )}
          {subscriber && obj.timeoutMode === "custom" && <label><span>{t("trdp.advanced.timeoutUs")}</span><input className={inputClass} type="number" min={1} value={obj.timeoutUs} onChange={event => patchObject(obj.id, { timeoutUs: Number(event.target.value) })} /></label>}
          <label className={styles.detailWide}><span>Payload HEX</span><textarea className={`${styles.detailTextarea} liquid-glass-input liquid-glass-textarea`} value={obj.payloadHex} onChange={event => patchObject(obj.id, { payloadHex: event.target.value.replace(/[^0-9a-f]/gi, "").toUpperCase() })} /></label>
        </div>

        <details className={styles.advancedPanel}>
          <summary className={styles.advancedSummary}><Icon name="chevron-right" size="xs" className={styles.advancedChevron} />{t("trdp.actions.advanced")}</summary>
          <div className={styles.objectDetailGrid}>
            <label><span>{t("trdp.advanced.source")}</span><input className={inputClass} value={obj.source} onChange={event => patchObject(obj.id, { source: event.target.value })} /></label>
            <label><span>ETB</span><input className={inputClass} type="number" min={0} value={obj.etbTopoCount} onChange={event => patchObject(obj.id, { etbTopoCount: Number(event.target.value) })} /></label>
            <label><span>OpTrn</span><input className={inputClass} type="number" min={0} value={obj.opTrnTopoCount} onChange={event => patchObject(obj.id, { opTrnTopoCount: Number(event.target.value) })} /></label>
            {subscriber && <label><span>{t("trdp.advanced.timeoutBehavior")}</span><select className={selectClass} value={obj.timeoutBehavior} onChange={event => patchObject(obj.id, { timeoutBehavior: event.target.value as "keep" | "zero" })}><option value="keep">{t("trdp.advanced.keepLast")}</option><option value="zero">{t("trdp.advanced.setZero")}</option></select></label>}
            {obj.kind === "pd_request" && <><label><span>{t("trdp.advanced.replyComId")}</span><input className={inputClass} type="number" min={0} value={obj.replyComId} onChange={event => patchObject(obj.id, { replyComId: Number(event.target.value) })} /></label><label><span>{t("trdp.advanced.replyIp")}</span><input className={inputClass} value={obj.replyIp} onChange={event => patchObject(obj.id, { replyIp: event.target.value })} /></label></>}
            {obj.kind === "pd_publisher" && <><label><span>{t("trdp.advanced.redId")}</span><input className={inputClass} type="number" min={0} value={obj.redId} onChange={event => patchObject(obj.id, { redId: Number(event.target.value) })} /></label><label><span>{t("trdp.advanced.redState")}</span><select className={selectClass} value={redundancyState} disabled={obj.redId <= 0} onChange={event => void updateRedundancyGroup(obj.redId, event.target.value as RedundancyState)}><option value="leader">{t("trdp.advanced.leader")}</option><option value="follower">{t("trdp.advanced.follower")}</option></select></label></>}
            {obj.kind.startsWith("md_") && <><label><span>{t("trdp.advanced.sourceUri")}</span><input className={inputClass} value={obj.sourceUri} onChange={event => patchObject(obj.id, { sourceUri: event.target.value })} /></label><label><span>{t("trdp.advanced.destinationUri")}</span><input className={inputClass} value={obj.destUri} onChange={event => patchObject(obj.id, { destUri: event.target.value })} /></label></>}
            {obj.kind === "md_request" && <><label><span>{t("trdp.advanced.replies")}</span><input className={inputClass} type="number" min={1} value={obj.numReplies} onChange={event => patchObject(obj.id, { numReplies: Number(event.target.value) })} /></label><label><span>{t("trdp.advanced.replyTimeout")}</span><input className={inputClass} type="number" min={1} value={obj.replyTimeoutUs} onChange={event => patchObject(obj.id, { replyTimeoutUs: Number(event.target.value) })} /></label></>}
            {obj.kind === "md_listener" && <><label><span>{t("trdp.advanced.response")}</span><select className={selectClass} value={obj.responseMode} onChange={event => patchObject(obj.id, { responseMode: event.target.value as "reply" | "query" })}><option value="reply">Reply (Mp)</option><option value="query">ReplyQuery (Mq)</option></select></label>{obj.responseMode === "query" && <label><span>{t("trdp.advanced.confirmTimeout")}</span><input className={inputClass} type="number" min={1} value={obj.confirmTimeoutUs} onChange={event => patchObject(obj.id, { confirmTimeoutUs: Number(event.target.value) })} /></label>}</>}
          </div>
        </details>
      </div>
    );
  }

  const visibleNav = mode === "monitor" ? [] : nav;
  const pdObjects = objects.filter(object => object.kind.startsWith("pd_")).filter(object => {
    if (pdFilter === "all") return true;
    if (pdFilter === "publisher") return object.kind === "pd_publisher";
    if (pdFilter === "subscriber") return object.kind === "pd_subscriber";
    return object.kind === "pd_request";
  });
  const mdObjects = objects.filter(object => object.kind.startsWith("md_")).filter(object => {
    if (mdFilter === "all") return true;
    if (mdFilter === "request") return object.kind === "md_request";
    if (mdFilter === "listener") return object.kind === "md_listener";
    return object.kind === "md_notify";
  });
  const selectedPdObject = pdObjects.find(object => object.id === selectedPdObjectId) ?? pdObjects[0];
  const selectedMdObject = mdObjects.find(object => object.id === selectedMdObjectId) ?? mdObjects[0];
  const subscriberFlows = flows.filter(flow => flow.msg.startsWith("P"));
  const packetRows = captureSource === "offline"
    ? pagedPackets
    : events.slice().reverse().slice(0, 1000);
  const packetPageCount = captureSource === "offline"
    ? Math.max(1, Math.ceil(capturePacketCount / PACKET_PAGE_SIZE))
    : 1;

  return (
    <div className={styles.root}>
      {visibleNav.length > 0 && (
        <div className={styles.navBar}>
          {visibleNav.map(([key, label]) => (
            <button
              key={key}
              className={styles.navButton + " liquid-glass-button " + (page === key ? "liquid-theme-selected" : "")}
              onClick={() => setPage(key)}
            >
              {t(label)}
            </button>
          ))}
        </div>
      )}

      <div className={styles.content}>
        {error && <div className={styles.error}>{error}</div>}

        {structuredEditor && structuredObject && structuredDataset && (
          <div className={styles.structuredEditor + " liquid-glass-card"}>
            <div className={styles.structuredHeader}>
              <strong>{t("trdp.structured.editor")} · {structuredDataset.name} ({structuredDataset.id})</strong>
              <span>{t("trdp.structured.object")}: {structuredObject.name} · ComID {structuredObject.comId}</span>
              <div className={styles.structuredActions}>
                <button className={styles.actionButton + " liquid-glass-button"} onClick={() => void decodeStructuredFromHex()}>{t("trdp.actions.hexToFields")}</button>
                <button className={styles.actionButton + " liquid-primary-button"} onClick={() => void applyStructuredToHex()}>{t("trdp.actions.fieldsToHex")}</button>
                <button className={styles.actionButton + " liquid-glass-button"} onClick={() => setStructuredEditor(null)}>{t("trdp.actions.close")}</button>
              </div>
            </div>
            <div className={styles.structuredHint}>
              {t("trdp.structured.hint")}
            </div>
            <table className={styles.table} style={{ marginTop: 8 }}>
              <thead><tr><th>{t("trdp.table.field")}</th><th>{t("trdp.table.type")}</th><th>{t("trdp.table.array")}</th><th>{t("trdp.table.valueJson")}</th><th>{t("trdp.table.unit")}</th></tr></thead>
              <tbody>
                {structuredDataset.elements.map(element => (
                  <tr key={element.name}>
                    <td>{element.name}</td>
                    <td>{element.data_type}{element.scale !== undefined ? " ×" + element.scale : ""}{element.offset !== undefined ? " +" + element.offset : ""}</td>
                    <td>{element.dynamic ? t("trdp.structured.dynamic") : element.array_size}</td>
                    <td>
                      <textarea
                        rows={1}
                        className={styles.textarea + " liquid-glass-input liquid-glass-textarea"}
                        value={structuredEditor.drafts[element.name] ?? "null"}
                        onChange={event => setStructuredEditor(prev => prev ? {
                          ...prev,
                          drafts: { ...prev.drafts, [element.name]: event.target.value },
                        } : prev)}
                      />
                    </td>
                    <td>{element.unit ?? "—"}</td>
                  </tr>
                ))}
              </tbody>
            </table>
            <div className={styles.payload}>{t("trdp.structured.payloadHex")}: <code>{structuredObject.payloadHex || "—"}</code></div>
          </div>
        )}

        {page === "overview" && (
          <section className={styles.section}>
            <div className={styles.sectionHeader}>
              <h2 className={styles.sectionTitle}>{t("trdp.nav.overview")}</h2>
            </div>

            <div className={styles.overviewInfo}>
              <div className={`${styles.infoCard} liquid-glass-card`}>
                <strong>{t("trdp.overview.protocol")}</strong><br />
                PD: UDP/{paramNumber(params, "pd_port", 17224)} · MD: UDP/{paramNumber(params, "md_udp_port", 17225)} TCP/{paramNumber(params, "md_tcp_port", 17225)} · SDT: {xmlImport?.sdt_detected ? t("trdp.overview.detectedNotValidated") : t("trdp.overview.validationNotPerformed")}
              </div>
              <div className={`${styles.infoCard} liquid-glass-card`}>
                <strong>{t("trdp.overview.links")}</strong><br />
                {t("trdp.overview.linkA")}: {String(params?.link_a_ip ?? "—")} · {t("trdp.overview.linkB")}: {params?.link_b_enabled ? String(params?.link_b_ip ?? "—") : t("trdpSidebar.disabled")}
              </div>
              <div className={`${styles.infoCard} liquid-glass-card`}>
                <strong>{t("trdp.overview.objects")}</strong><br />
                PD {objects.filter(object => object.kind.startsWith("pd_")).length} · MD {objects.filter(object => object.kind.startsWith("md_")).length} · {t("trdp.overview.activeObjects")} {objects.filter(object => object.state === "running").length}
              </div>
              <div className={`${styles.infoCard} liquid-glass-card`}>
                <strong>{t("trdp.overview.txPolicy")}</strong><br />
                {t("trdp.overview.txPolicyText")}
              </div>
              <div className={`${styles.infoCard} liquid-glass-card`}>
                <strong>{t("trdp.overview.safety")}</strong><br />
                {t("trdp.overview.safetyText")}
              </div>
              {workspaceName && <div className={`${styles.infoCard} ${styles.overviewWorkspaceCard} liquid-glass-card`}><strong>{t("trdp.overview.workspace")}</strong><br />{workspaceName} · {t("trdp.overview.importedStopped")}</div>}
            </div>

            <div className={styles.toolbar}>
              <button className={`${styles.actionButton} liquid-glass-button`} onClick={() => void importXml()}>{t("trdp.actions.importXml")}</button>
              <button className={`${styles.actionButton} liquid-glass-button`} onClick={() => void importWorkspace()}>{t("trdp.actions.importWorkspace")}</button>
            </div>

            {xmlImport && (
              <div className={`${styles.infoCard} liquid-glass-card`}>
                <strong>{t("trdp.overview.importPreview")}</strong>
                <div>{xmlImport.datasets.length} {t("trdp.overview.datasets")} · {xmlImport.telegrams.length} {t("trdp.overview.telegrams")} · {t("trdp.overview.ports")} {xmlImport.pd_port}/{xmlImport.md_udp_port}/{xmlImport.md_tcp_port} · SDT: {xmlImport.sdt_detected ? t("trdp.overview.detectedNotValidated") : t("trdp.overview.noConfigDetected")}</div>
                {xmlImport.warnings.map(warning => <div key={warning} className={styles.warningLine}>⚠ {warning}</div>)}
                <button className={`${styles.actionButton} liquid-glass-button`} onClick={importTemplates}>{t("trdp.actions.importTemplates")}</button>
                <div className={styles.tableWrap}>
                  <table className={styles.table}>
                    <thead><tr><th>{t("trdp.table.type")}</th><th>Telegram</th><th>ComID</th><th>Dataset</th><th>{t("trdp.table.cycle")}</th><th>{t("trdp.table.timeout")}</th><th>{t("trdp.table.sources")}</th><th>{t("trdp.table.destinations")}</th></tr></thead>
                    <tbody>
                      {xmlImport.telegrams.length === 0 ? (
                        <tr><td colSpan={8} className={styles.emptyState}>{t("trdp.empty.xmlNoTelegram")}</td></tr>
                      ) : xmlImport.telegrams.map(telegram => (
                        <tr key={`${telegram.com_id}-${telegram.name}`}>
                          <td>{telegram.traffic_kind.toUpperCase()}</td><td>{telegram.name}</td><td>{telegram.com_id}</td><td>{telegram.dataset_id}</td><td>{telegram.cycle_us ?? "—"}</td>
                          <td>{telegram.traffic_kind === "pd" ? (telegram.timeout_us && telegram.timeout_us > 0 ? `${telegram.timeout_us} µs / ${t(`trdp.status.${telegram.timeout_behavior ?? "zero"}`)}` : `${t("trdp.status.disabled")} / ${t(`trdp.status.${telegram.timeout_behavior ?? "zero"}`)}`) : "—"}</td>
                          <td>{telegram.sources.join(", ") || "—"}</td><td>{telegram.destinations.join(", ") || "—"}</td>
                        </tr>
                      ))}
                    </tbody>
                  </table>
                </div>
              </div>
            )}
          </section>
        )}

        {page === "pd" && (
          <section className={styles.section}>
            <div className={styles.sectionHeader}>
              <h2 className={styles.sectionTitle}>{t("trdp.nav.pd")}</h2>
              <button className={`${styles.actionButton} liquid-glass-button`} onClick={() => addObject("pd_publisher")}>{t("trdp.actions.addPublisher")}</button>
              <button className={`${styles.actionButton} liquid-glass-button`} onClick={() => addObject("pd_subscriber")}>{t("trdp.actions.addSubscriber")}</button>
              <button className={`${styles.actionButton} liquid-glass-button`} onClick={() => addObject("pd_request")}>{t("trdp.actions.addPdRequest")}</button>
            </div>
            <div className={styles.filterBar}>
              {(["all", "publisher", "subscriber", "request"] as const).map(filter => (
                <button key={filter} className={`${styles.filterButton} liquid-glass-button ${pdFilter === filter ? "liquid-theme-selected" : ""}`} onClick={() => setPdFilter(filter)}>{t(`trdp.filter.${filter}`)}</button>
              ))}
            </div>
            {objectSummaryTable(pdObjects, selectedPdObject?.id ?? null, setSelectedPdObjectId)}
            <h3 className={styles.subheading}>{t("trdp.section.objectDetails")}</h3>
            {renderObjectDetail(selectedPdObject)}
            <h3 className={styles.subheading}>{t("trdp.section.subscriberDiagnostics")}</h3>
            <div className={styles.tableWrap}>
              <table className={`${styles.table} ${styles.diagnosticTable}`}>
                <thead><tr><th>{t("trdp.table.link")}</th><th>ComID</th><th>{t("trdp.table.packets")}</th><th>{t("trdp.table.missedSeq")}</th><th>{t("trdp.table.lastSeq")}</th><th>{t("trdp.table.interval")}</th><th>{t("trdp.table.avgJitter")}</th><th>{t("trdp.table.errors")}</th></tr></thead>
                <tbody>{subscriberFlows.length === 0 ? <tr><td colSpan={8} className={styles.emptyState}>{t("trdp.empty.noPdTraffic")}</td></tr> : subscriberFlows.map(flow => <tr key={`diag-${flow.key}`}><td>{flow.link}</td><td>{flow.comId}</td><td>{flow.count}</td><td>{flow.missed}</td><td>{flow.lastSeq ?? "—"}</td><td>{flow.minIntervalUs === undefined ? "—" : `${Math.round(flow.minIntervalUs)}/${Math.round(flow.avgIntervalUs ?? 0)}/${Math.round(flow.maxIntervalUs ?? 0)}`}</td><td>{flow.jitterUs === undefined ? "—" : Math.round(flow.jitterUs)}</td><td>{flow.errors}</td></tr>)}</tbody>
              </table>
            </div>
          </section>
        )}

        {page === "md" && (
          <section className={styles.section}>
            <div className={styles.sectionHeader}>
              <h2 className={styles.sectionTitle}>{t("trdp.nav.md")}</h2>
              <button className={`${styles.actionButton} liquid-glass-button`} onClick={() => addObject("md_request")}>{t("trdp.actions.addRequest")}</button>
              <button className={`${styles.actionButton} liquid-glass-button`} onClick={() => addObject("md_listener")}>{t("trdp.actions.addListener")}</button>
              <button className={`${styles.actionButton} liquid-glass-button`} onClick={() => addObject("md_notify")}>{t("trdp.actions.addNotify")}</button>
            </div>
            <div className={styles.filterBar}>
              {(["all", "request", "listener", "notify"] as const).map(filter => (
                <button key={filter} className={`${styles.filterButton} liquid-glass-button ${mdFilter === filter ? "liquid-theme-selected" : ""}`} onClick={() => setMdFilter(filter)}>{t(`trdp.filter.${filter}`)}</button>
              ))}
            </div>
            {objectSummaryTable(mdObjects, selectedMdObject?.id ?? null, setSelectedMdObjectId)}
            <h3 className={styles.subheading}>{t("trdp.section.objectDetails")}</h3>
            {renderObjectDetail(selectedMdObject)}
          </section>
        )}

        {page === "analysis" && (
          <section className={`${styles.section} ${styles.analysisSection}`}>
            <div className={styles.sectionHeader}>
              <h2 className={styles.sectionTitle}>{t("trdp.nav.analysis")}</h2>
            </div>

            <div className={`${styles.captureSourceCard} liquid-glass-card`}>
              <div className={styles.captureSourceHeader}>
                <div>
                  <strong>{t("trdp.section.captureSource")}</strong>
                  <span>{mode === "monitor" ? t("trdp.analysis.monitorHint") : t("trdp.analysis.nodeHint")}</span>
                </div>
                <div className={styles.toolbar}>
                  {mode === "monitor" && <button className={`${styles.actionButton} ${liveCaptureSetupOpen ? "liquid-theme-selected" : "liquid-glass-button"}`} onClick={() => void toggleLiveCaptureSetup()}>{t("trdp.actions.liveCaptureSettings")}</button>}
                  <button className={`${styles.actionButton} liquid-glass-button`} onClick={() => void openCapture()} disabled={captureRunning || captureTransitioning}>{t("trdp.actions.openCapture")}</button>
                  <button className={`${styles.actionButton} liquid-glass-button`} onClick={() => void importXml()}>{t("trdp.actions.importXml")}</button>
                  <button className={`${styles.actionButton} liquid-glass-button`} onClick={() => void saveCapture()} disabled={!captureId}>{t("trdp.actions.saveCapture")}</button>
                  <button className={`${styles.actionButton} liquid-glass-button`} onClick={clearCaptureView} disabled={captureRunning || captureTransitioning}>{t("trdp.actions.clear")}</button>
                </div>
              </div>
              <div className={styles.captureStatus}>
                {captureRunning ? t("trdp.overview.running") : captureSource ? t("trdp.overview.stopped") : t("trdp.overview.notStarted")}
                {captureSource ? <> · {t("trdp.overview.source")} {t(`trdp.overview.${captureSource}`)}</> : null}
                {" · "}{t("trdp.overview.bufferedFrames")} {captureFrameCount}
                {" · "}{t("trdp.table.packets")} {capturePacketCount}
                {captureDroppedFrames > 0 ? <> · ⚠ {captureDroppedFrames} {t("trdp.overview.droppedFrames")} ({LIVE_CAPTURE_FRAME_LIMIT.toLocaleString()})</> : null}
              </div>

              {mode === "monitor" && liveCaptureSetupOpen && (
                <div className={styles.liveCaptureSetup}>
                  <div className={styles.objectDetailGrid}>
                    <label><span>{t("trdp.form.captureInterfaceA")}</span><select className={`${styles.detailInput} liquid-glass-input liquid-glass-select`} value={captureInterfaceA} onChange={event => { const next = event.target.value; setCaptureInterfaceA(next); if (next === captureInterfaceB) setCaptureInterfaceB(""); }} disabled={captureInterfacesLoading}><option value="">{captureInterfacesLoading ? t("trdp.captureInterfaces.loading") : t("trdp.captureInterfaces.choose")}</option>{captureInterfaces.map(item => <option key={item.name} value={item.name}>{item.description ? `${item.description} — ${item.name}` : item.name}</option>)}</select></label>
                    <label className={styles.toggleField}><span>{t("trdp.form.captureLinkB")}</span><span className="liquid-glass-toggle"><input type="checkbox" checked={captureInterfaceBEnabled} onChange={event => setCaptureInterfaceBEnabled(event.target.checked)} /><div /></span></label>
                    {captureInterfaceBEnabled && <label><span>{t("trdp.form.captureInterfaceB")}</span><select className={`${styles.detailInput} liquid-glass-input liquid-glass-select`} value={captureInterfaceB} onChange={event => setCaptureInterfaceB(event.target.value)} disabled={captureInterfacesLoading}><option value="">{t("trdp.captureInterfaces.choose")}</option>{captureInterfaces.filter(item => item.name !== captureInterfaceA).map(item => <option key={item.name} value={item.name}>{item.description ? `${item.description} — ${item.name}` : item.name}</option>)}</select></label>}
                    <label className={styles.toggleField}><span>{t("trdp.form.autoFilter")}</span><span className="liquid-glass-toggle"><input type="checkbox" checked={captureFilterAuto} onChange={event => setCaptureFilterAuto(event.target.checked)} /><div /></span></label>
                    <label className={styles.detailWide}><span>{t("trdp.form.captureFilter")}</span>{captureFilterAuto ? <code className={styles.filterPreview}>{captureFilterForPorts(paramNumber(params, "pd_port", 17224), paramNumber(params, "md_udp_port", 17225), paramNumber(params, "md_tcp_port", 17225))}</code> : <input className={`${styles.detailInput} liquid-glass-input`} value={captureFilter} onChange={event => setCaptureFilter(event.target.value)} />}</label>
                  </div>
                  <div className={styles.toolbar}>
                    <button className={`${styles.actionButton} liquid-glass-button`} onClick={() => void refreshCaptureInterfaces()} disabled={captureInterfacesLoading}>{t("trdp.actions.refreshInterfaces")}</button>
                    {captureRunning ? (
                      <button className={`${styles.actionButton} liquid-glass-button`} onClick={() => void stopLiveCapture()} disabled={captureTransitioning || !sessionConnected}>{t("trdp.actions.stopCapture")}</button>
                    ) : (
                      <button className={`${styles.actionButton} liquid-primary-button`} onClick={() => void startLiveCapture()} disabled={captureInterfacesLoading || captureTransitioning || !captureInterfaceA || !sessionConnected}>{t("trdp.actions.startCapture")}</button>
                    )}
                  </div>
                </div>
              )}
            </div>

            <TrdpAnalysisTables
              flows={flows}
              packetRows={packetRows}
              selectedPacket={selectedPacket}
              packetTotal={captureId ? capturePacketCount : events.length}
              packetPage={packetPage}
              packetPageCount={packetPageCount}
              packetPageSize={PACKET_PAGE_SIZE}
              onPacketPageChange={pageIndex => { void loadOfflinePacketPage(pageIndex); }}
              onInspectPacket={event => { void inspectPacket(event); }}
            />

            {selectedPacket && (
              <TrdpPacketInspector
                selectedPacket={selectedPacket}
                decoded={decoded}
                xmlImport={xmlImport}
                onConfirmMessage={event => { void confirmMessage(event); }}
                canConfirmMessage={sessionConnected && selectedPacket.can_confirm === true}
                mdLatencyUs={mdLatencyUs}
                observedMdReplies={observedMdReplies}
              />
            )}
          </section>
        )}
      </div>
    </div>
  );
}
