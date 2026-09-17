import { useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { open, save } from "@tauri-apps/plugin-dialog";
import { useTranslation } from "react-i18next";
import { useSession } from "../../context/SessionContext";
import styles from "./TrdpSessionView.module.css";
import { TrdpAnalysisTables, TrdpPacketInspector } from "./TrdpPanels";
import {
  LIVE_CAPTURE_FRAME_LIMIT,
  STANDARD_CAPTURE_FILTER,
  captureFilterForPorts,
  missedBetween,
  monitorCaptureInterfaces,
  paramNumber,
  type CaptureFlowSummary,
  type CaptureResult,
  type CaptureSummary,
  type DecodedDataset,
  type FlowRow,
  type Page,
  type RuntimeState,
  type TrdpEvent,
  type XmlImport,
} from "./model";

const PACKET_PAGE_SIZE = 250;
const MONITOR_NAV: Array<[Page, string]> = [
  ["overview", "trdp.nav.overview"],
  ["pd", "trdp.nav.pd"],
  ["md", "trdp.nav.md"],
  ["analysis", "trdp.nav.analysis"],
];

type CaptureSource = "offline" | "live" | null;

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

function isPdEvent(event: TrdpEvent) {
  return event.kind === "pd" || event.msg_type?.startsWith("P") === true;
}

function isMdEvent(event: TrdpEvent) {
  return event.kind === "md" || event.msg_type?.startsWith("M") === true;
}

export default function TrdpMonitorView({ sessionId }: { sessionId: string }) {
  const { t } = useTranslation();
  const { state } = useSession();
  const tab = state.tabs.find(item => item.id === sessionId);
  const params = tab?.params as Record<string, unknown> | undefined;
  const sessionConnected = tab?.state === "connected" || tab?.state === "transferring";

  const [page, setPage] = useState<Page>("overview");
  const [error, setError] = useState<string | null>(null);
  const [xmlImport, setXmlImport] = useState<XmlImport | null>(null);
  const [decoded, setDecoded] = useState<DecodedDataset | null>(null);
  const [selectedPacket, setSelectedPacket] = useState<TrdpEvent | null>(null);

  const [captureId, setCaptureId] = useState<string | null>(null);
  const captureIdRef = useRef<string | null>(null);
  const [captureSource, setCaptureSource] = useState<CaptureSource>(null);
  const captureSourceRef = useRef<CaptureSource>(null);
  const [captureRunning, setCaptureRunning] = useState(false);
  const captureRunningRef = useRef(false);
  const [captureTransitioning, setCaptureTransitioning] = useState(false);
  const captureTransitioningRef = useRef(false);
  const viewMountedRef = useRef(true);
  const [captureFrameCount, setCaptureFrameCount] = useState(0);
  const [capturePacketCount, setCapturePacketCount] = useState(0);
  const [captureDroppedFrames, setCaptureDroppedFrames] = useState(0);
  const [captureFlows, setCaptureFlows] = useState<FlowRow[]>([]);
  const [packetPage, setPacketPage] = useState(0);
  const [pagedPackets, setPagedPackets] = useState<TrdpEvent[]>([]);
  const [events, setEvents] = useState<TrdpEvent[]>([]);

  const packetBatchRef = useRef<TrdpEvent[]>([]);
  const batchTimerRef = useRef<number | null>(null);
  const summaryTimerRef = useRef<number | null>(null);
  const mdRequestStartedUs = useRef(new Map<string, number>());

  const pdPort = paramNumber(params, "pd_port", 17224);
  const mdUdpPort = paramNumber(params, "md_udp_port", 17225);
  const mdTcpPort = paramNumber(params, "md_tcp_port", 17225);
  const captureConfig = monitorCaptureInterfaces(params);
  const captureInterfaceA = captureConfig.a;
  const captureInterfaceB = captureConfig.b;
  const captureInterfaceBEnabled = captureInterfaceB !== null;
  const configuredFilter = typeof params?.capture_filter === "string"
    ? params.capture_filter
    : STANDARD_CAPTURE_FILTER;
  const captureFilterAuto = typeof params?.capture_filter_auto === "boolean"
    ? params.capture_filter_auto
    : configuredFilter === STANDARD_CAPTURE_FILTER;
  const effectiveCaptureFilter = captureFilterAuto
    ? captureFilterForPorts(pdPort, mdUdpPort, mdTcpPort)
    : configuredFilter;

  function updateCaptureRunning(value: boolean) {
    captureRunningRef.current = value;
    setCaptureRunning(value);
  }

  function updateCaptureTransitioning(value: boolean) {
    captureTransitioningRef.current = value;
    setCaptureTransitioning(value);
  }

  function updateCaptureSource(value: CaptureSource) {
    captureSourceRef.current = value;
    setCaptureSource(value);
  }

  function selectPage(nextPage: Page) {
    setPage(nextPage);
    setSelectedPacket(null);
    setDecoded(null);
  }

  function adoptCapture(nextCaptureId: string | null) {
    captureIdRef.current = nextCaptureId;
    setCaptureId(nextCaptureId);
  }

  function resetCapturePresentation() {
    adoptCapture(null);
    updateCaptureSource(null);
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
    mdRequestStartedUs.current.clear();
  }

  useEffect(() => {
    if (sessionConnected) return;
    updateCaptureRunning(false);
    updateCaptureTransitioning(false);
    if (captureSourceRef.current === "live") {
      resetCapturePresentation();
    }
  }, [sessionConnected]);

  useEffect(() => {
    viewMountedRef.current = true;
    return () => {
      viewMountedRef.current = false;
      if (captureSourceRef.current === "offline") {
        const current = captureIdRef.current;
        if (current) void invoke("trdp_release_capture", { captureId: current });
      }
    };
  }, [sessionId]);

  useEffect(() => {
    if (!sessionConnected || captureSourceRef.current === "offline") return;
    let disposed = false;
    void invoke<RuntimeState>("trdp_command", {
      sessionId,
      command: { command: "runtime_state" },
    }).then(runtime => {
      if (disposed || !runtime.capture?.id) return;
      const captureRuntime = runtime.capture;
      adoptCapture(captureRuntime.id);
      updateCaptureSource("live");
      updateCaptureRunning(captureRuntime.running);
      return invoke<CaptureSummary>("trdp_capture_summary", {
        captureId: captureRuntime.id,
      }).then(summary => {
        if (disposed || captureIdRef.current !== captureRuntime.id) return;
        setCapturePacketCount(summary.packet_count);
        setCaptureFlows(summary.flows.map(flowRowFromSummary));
      });
    }).catch(() => {
      // Runtime hydration is best-effort; explicit user actions still surface errors.
    });
    return () => {
      disposed = true;
    };
  }, [sessionConnected, sessionId]);

  useEffect(() => {
    let disposed = false;

    const flushBatches = () => {
      batchTimerRef.current = null;
      const packets = packetBatchRef.current.splice(0);
      if (packets.length > 0) {
        setEvents(previous => [...previous, ...packets].slice(-5000));
      }
    };

    const scheduleFlush = () => {
      if (batchTimerRef.current !== null) return;
      batchTimerRef.current = window.setTimeout(flushBatches, 40);
    };

    const unlisten = listen<TrdpEvent>("trdp-event", ({ payload }) => {
      if (disposed || payload.session_id !== sessionId) return;

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
              // A stopped/released capture can race with the throttled refresh.
            });
          }, 250);
        }
      }

      if (payload.event === "packet") {
        let packet = payload;
        if (payload.md_session_id && payload.timestamp_us !== undefined) {
          if (payload.msg_type === "Mr") {
            mdRequestStartedUs.current.set(payload.md_session_id, payload.timestamp_us);
          } else if (["Mp", "Mq", "Me"].includes(payload.msg_type ?? "")) {
            const started = mdRequestStartedUs.current.get(payload.md_session_id);
            if (started !== undefined && payload.timestamp_us >= started) {
              packet = { ...payload, latency_us: payload.timestamp_us - started };
            }
          }
          if (payload.about_to_die) {
            mdRequestStartedUs.current.delete(payload.md_session_id);
          }
          if (mdRequestStartedUs.current.size > 1024) {
            const oldest = mdRequestStartedUs.current.keys().next().value;
            if (typeof oldest === "string") mdRequestStartedUs.current.delete(oldest);
          }
        }
        packetBatchRef.current.push(packet);
        scheduleFlush();
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
    for (const telegram of xmlImport?.telegrams ?? []) {
      result.set(telegram.com_id, telegram.dataset_id);
    }
    return result;
  }, [xmlImport]);

  const expectedCycleByComId = useMemo(() => {
    const result = new Map<number, number>();
    for (const telegram of xmlImport?.telegrams ?? []) {
      if (telegram.cycle_us) result.set(telegram.com_id, telegram.cycle_us);
    }
    return result;
  }, [xmlImport]);

  const eventFlows = useMemo(() => {
    type MutableFlow = FlowRow & {
      intervals: number[];
      previousSeq?: number;
      previousTimestamp?: number;
    };
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
        if (row.previousSeq !== undefined) {
          row.missed += missedBetween(row.previousSeq, event.seq_count);
        }
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
      if (row.intervals.length > 0) {
        row.minIntervalUs = Math.min(...row.intervals);
        row.maxIntervalUs = Math.max(...row.intervals);
        row.avgIntervalUs = row.intervals.reduce((sum, value) => sum + value, 0) / row.intervals.length;
        if (expected !== undefined) {
          row.jitterUs = row.intervals.reduce(
            (sum, value) => sum + Math.abs(value - expected),
            0,
          ) / row.intervals.length;
        }
      }
      const {
        intervals: _intervals,
        previousSeq: _previousSeq,
        previousTimestamp: _previousTimestamp,
        ...result
      } = row;
      return result;
    });
  }, [events, expectedCycleByComId]);

  const flows = captureId ? captureFlows : eventFlows;
  const pdFlows = flows.filter(flow => flow.msg.startsWith("P"));
  const mdFlows = flows.filter(flow => flow.msg.startsWith("M"));
  const packetRows = captureSource === "offline"
    ? pagedPackets
    : events.slice().reverse().slice(0, 1000);
  const pdPacketRows = packetRows.filter(isPdEvent);
  const mdPacketRows = packetRows.filter(isMdEvent);
  const packetPageCount = captureSource === "offline"
    ? Math.max(1, Math.ceil(capturePacketCount / PACKET_PAGE_SIZE))
    : 1;
  const pdPacketCount = pdFlows.reduce((sum, flow) => sum + flow.count, 0);
  const mdPacketCount = mdFlows.reduce((sum, flow) => sum + flow.count, 0);

  async function command<T = unknown>(name: string, payload: Record<string, unknown> = {}): Promise<T> {
    setError(null);
    try {
      return await invoke<T>("trdp_command", {
        sessionId,
        command: { command: name, ...payload },
      });
    } catch (cause) {
      setError(String(cause));
      throw cause;
    }
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
    const pageIndex = Math.max(0, Math.min(nextPage, pageCount - 1));
    const remaining = Math.max(0, total - pageIndex * PACKET_PAGE_SIZE);
    const count = Math.min(PACKET_PAGE_SIZE, remaining);
    const offset = Math.max(0, remaining - count);
    const packets = await invoke<TrdpEvent[]>("trdp_capture_packets", {
      captureId: id,
      offset,
      limit: count,
    });
    setPacketPage(pageIndex);
    setPagedPackets(packets.slice().reverse());
    setSelectedPacket(null);
    setDecoded(null);
  }

  async function releaseCurrentCapture() {
    const current = captureIdRef.current;
    const source = captureSourceRef.current;
    if (!current || !source) return;
    if (source === "live") {
      await command("capture_release");
    } else {
      await invoke("trdp_release_capture", { captureId: current });
    }
  }

  async function openCapture() {
    if (captureRunning || captureTransitioning) return;
    setError(null);
    const path = await open({
      multiple: false,
      filters: [{ name: "Packet Capture", extensions: ["pcap", "pcapng"] }],
    });
    if (typeof path !== "string") return;
    try {
      const mdPorts = [...new Set([mdUdpPort, mdTcpPort])];
      const result = await invoke<CaptureResult>("trdp_open_capture", {
        path,
        pdPorts: [pdPort],
        mdPorts,
        expectedCycles: Object.fromEntries(expectedCycleByComId),
      });
      try {
        await releaseCurrentCapture();
      } catch (cause) {
        await invoke("trdp_release_capture", { captureId: result.capture_id });
        throw cause;
      }
      adoptCapture(result.capture_id);
      updateCaptureSource("offline");
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
      selectPage("analysis");
      await loadOfflinePacketPage(0, result.capture_id, result.packet_count);
    } catch (cause) {
      setError(String(cause));
    }
  }

  async function saveCapture() {
    if (!captureId) return;
    setError(null);
    const path = await save({ filters: [{ name: "PCAPNG", extensions: ["pcapng"] }] });
    if (!path) return;
    try {
      await invoke("trdp_save_capture", { path, captureId });
    } catch (cause) {
      setError(String(cause));
    }
  }

  async function importXml() {
    setError(null);
    const selected = await open({
      multiple: false,
      filters: [{ name: "TRDP XML", extensions: ["xml"] }],
    });
    if (typeof selected !== "string") return;
    try {
      const imported = await command<XmlImport>("xml_import", { path: selected });
      setXmlImport(imported);
      setDecoded(null);
    } catch {
      // command() owns the error banner.
    }
  }

  async function clearCaptureView() {
    if (captureRunning || captureTransitioning) return;
    updateCaptureTransitioning(true);
    try {
      await releaseCurrentCapture();
      resetCapturePresentation();
    } catch {
      // releaseCurrentCapture()/command() owns the error banner.
    } finally {
      updateCaptureTransitioning(false);
    }
  }

  async function startLiveCapture() {
    if (captureRunning || captureTransitioning) return;
    if (!sessionConnected) {
      setError(t("trdp.errors.connectRequired"));
      return;
    }
    if (!captureInterfaceA) {
      setError(t("trdp.captureInterfaces.choose"));
      return;
    }
    if (captureInterfaceB && captureInterfaceB.deviceName === captureInterfaceA.deviceName) {
      setError(`${t("trdp.form.captureInterfaceB")}: ${t("trdp.captureInterfaces.choose")}`);
      return;
    }

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
    mdRequestStartedUs.current.clear();

    try {
      const result = await command<{ capture_id: string }>("capture_start", {
        interface: captureInterfaceA.deviceName,
        interface_b: captureInterfaceB?.deviceName ?? "",
        filter: effectiveCaptureFilter,
        expected_cycles: Object.fromEntries(expectedCycleByComId),
      });
      if (previous.source === "offline" && previous.captureId) {
        await invoke("trdp_release_capture", { captureId: previous.captureId });
      }
      if (!viewMountedRef.current) return;
      adoptCapture(result.capture_id);
      updateCaptureSource("live");
      updateCaptureRunning(true);
    } catch {
      if (viewMountedRef.current) {
        captureIdRef.current = previous.captureId;
        setCaptureId(previous.captureId);
        updateCaptureSource(previous.source);
        updateCaptureRunning(previous.running);
        setCaptureFrameCount(previous.frameCount);
        setCapturePacketCount(previous.packetCount);
        setCaptureDroppedFrames(previous.droppedFrames);
        setCaptureFlows(previous.flows);
        setPacketPage(previous.packetPage);
        setPagedPackets(previous.pagedPackets);
        setEvents(previous.events);
      }
    } finally {
      if (viewMountedRef.current) updateCaptureTransitioning(false);
    }
  }

  async function stopLiveCapture() {
    if (!captureRunning || captureTransitioning) return;
    if (!sessionConnected) {
      updateCaptureRunning(false);
      return;
    }
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
      // command() owns the error banner; retain running state so the user can retry.
    } finally {
      updateCaptureTransitioning(false);
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
    } catch {
      // command() owns the error banner.
    }
  }

  function mdLatencyUs(event: TrdpEvent) {
    if (event.latency_us !== undefined) return event.latency_us;
    if (
      !event.md_session_id
      || event.timestamp_us === undefined
      || !["Mp", "Mq", "Me"].includes(event.msg_type ?? "")
    ) {
      return undefined;
    }
    const requests = events.filter(candidate =>
      candidate.md_session_id === event.md_session_id
      && candidate.msg_type === "Mr"
      && candidate.timestamp_us !== undefined
      && candidate.timestamp_us <= event.timestamp_us!,
    );
    const request = requests.length > 0 ? requests[requests.length - 1] : undefined;
    const started = request?.timestamp_us ?? mdRequestStartedUs.current.get(event.md_session_id);
    return started === undefined || event.timestamp_us < started
      ? undefined
      : event.timestamp_us - started;
  }

  function observedMdReplies(event: TrdpEvent) {
    if (!event.md_session_id) return undefined;
    return events.filter(candidate =>
      candidate.md_session_id === event.md_session_id
      && ["Mp", "Mq"].includes(candidate.msg_type ?? ""),
    ).length;
  }

  const renderPacketInspector = () => selectedPacket ? (
    <TrdpPacketInspector
      selectedPacket={selectedPacket}
      decoded={decoded}
      xmlImport={xmlImport}
      onConfirmMessage={() => {}}
      canConfirmMessage={false}
      mdLatencyUs={mdLatencyUs}
      observedMdReplies={observedMdReplies}
    />
  ) : null;

  const renderTrafficTables = (trafficFlows: FlowRow[], trafficPackets: TrdpEvent[]) => (
    <>
      <TrdpAnalysisTables
        flows={trafficFlows}
        packetRows={trafficPackets}
        selectedPacket={selectedPacket}
        packetTotal={captureId ? capturePacketCount : events.length}
        packetPage={packetPage}
        packetPageCount={packetPageCount}
        packetPageSize={PACKET_PAGE_SIZE}
        onPacketPageChange={pageIndex => { void loadOfflinePacketPage(pageIndex); }}
        onInspectPacket={event => { void inspectPacket(event); }}
      />
      {renderPacketInspector()}
    </>
  );

  return (
    <div className={styles.root}>
      <div className={styles.navBar}>
        {MONITOR_NAV.map(([key, label]) => (
          <button
            key={key}
            className={`${styles.navButton} liquid-glass-button ${page === key ? "liquid-theme-selected" : ""}`}
            onClick={() => selectPage(key)}
          >
            {t(label)}
          </button>
        ))}
      </div>

      <div className={styles.content}>
        {error && <div className={styles.error}>{error}</div>}

        <section className={styles.section}>
          <div className={`${styles.captureSourceCard} liquid-glass-card`}>
            <div className={styles.captureSourceHeader}>
              <div>
                <strong>{t("trdp.section.captureSource")}</strong>
                <span>{t("trdp.analysis.monitorHint")}</span>
              </div>
              <div className={styles.toolbar}>
                {captureRunning ? (
                  <button
                    className={`${styles.actionButton} liquid-glass-button`}
                    onClick={() => void stopLiveCapture()}
                    disabled={captureTransitioning}
                  >
                    {t("trdp.actions.stopCapture")}
                  </button>
                ) : (
                  <button
                    className={`${styles.actionButton} liquid-primary-button`}
                    onClick={() => void startLiveCapture()}
                    disabled={
                      captureTransitioning
                      || !sessionConnected
                      || !captureInterfaceA
                    }
                  >
                    {t("trdp.actions.startCapture")}
                  </button>
                )}
                <button
                  className={`${styles.actionButton} liquid-glass-button`}
                  onClick={() => void openCapture()}
                  disabled={captureRunning || captureTransitioning}
                >
                  {t("trdp.actions.openCapture")}
                </button>
                <button
                  className={`${styles.actionButton} liquid-glass-button`}
                  onClick={() => void importXml()}
                >
                  {t("trdp.actions.importXml")}
                </button>
                <button
                  className={`${styles.actionButton} liquid-glass-button`}
                  onClick={() => void saveCapture()}
                  disabled={!captureId}
                >
                  {t("trdp.actions.saveCapture")}
                </button>
                <button
                  className={`${styles.actionButton} liquid-glass-button`}
                  onClick={() => void clearCaptureView()}
                  disabled={captureRunning || captureTransitioning}
                >
                  {t("trdp.actions.clear")}
                </button>
              </div>
            </div>
            <div className={styles.captureStatus}>
              {captureRunning
                ? t("trdp.overview.running")
                : captureSource
                  ? t("trdp.overview.stopped")
                  : t("trdp.overview.notStarted")}
              {captureSource ? <> · {t("trdp.overview.source")} {t(`trdp.overview.${captureSource}`)}</> : null}
              {" · "}{t("trdp.overview.bufferedFrames")} {captureFrameCount}
              {" · "}{t("trdp.table.packets")} {capturePacketCount}
              {captureDroppedFrames > 0 ? (
                <> · ⚠ {captureDroppedFrames} {t("trdp.overview.droppedFrames")} ({LIVE_CAPTURE_FRAME_LIMIT.toLocaleString()})</>
              ) : null}
            </div>
          </div>

          {page === "overview" && (
            <>
              <div className={styles.sectionHeader}>
                <h2 className={styles.sectionTitle}>{t("trdp.nav.overview")}</h2>
              </div>
              <div className={styles.overviewInfo}>
                <div className={`${styles.infoCard} liquid-glass-card`}>
                  <strong>{t("trdp.overview.protocol")}</strong><br />
                  PD: UDP/{pdPort} · MD: UDP/{mdUdpPort} TCP/{mdTcpPort} · SDT: {xmlImport?.sdt_detected ? t("trdp.overview.detectedNotValidated") : t("trdp.overview.validationNotPerformed")}
                </div>
                <div className={`${styles.infoCard} liquid-glass-card`}>
                  <strong>{t("trdp.overview.links")}</strong><br />
                  {t("trdp.overview.linkA")}: {captureInterfaceA?.displayName || t("trdpSidebar.unconfigured")} · {t("trdp.overview.linkB")}: {captureInterfaceBEnabled ? captureInterfaceB.displayName : t("trdpSidebar.disabled")}
                </div>
                <div className={`${styles.infoCard} liquid-glass-card`}>
                  <strong>{t("trdp.form.captureFilter")}</strong><br />
                  <code>{effectiveCaptureFilter}</code>
                </div>
                <div className={`${styles.infoCard} liquid-glass-card`}>
                  <strong>{t("trdp.nav.pd")}</strong><br />
                  {t("trdp.section.flows")} {pdFlows.length} · {t("trdp.table.packets")} {pdPacketCount}
                </div>
                <div className={`${styles.infoCard} liquid-glass-card`}>
                  <strong>{t("trdp.nav.md")}</strong><br />
                  {t("trdp.section.flows")} {mdFlows.length} · {t("trdp.table.packets")} {mdPacketCount}
                </div>
                <div className={`${styles.infoCard} liquid-glass-card`}>
                  <strong>{t("trdp.overview.safety")}</strong><br />
                  {t("trdp.analysis.monitorHint")}
                </div>
              </div>
            </>
          )}

          {page === "pd" && (
            <>
              <div className={styles.sectionHeader}>
                <h2 className={styles.sectionTitle}>{t("trdp.nav.pd")}</h2>
              </div>
              {renderTrafficTables(pdFlows, pdPacketRows)}
            </>
          )}

          {page === "md" && (
            <>
              <div className={styles.sectionHeader}>
                <h2 className={styles.sectionTitle}>{t("trdp.nav.md")}</h2>
              </div>
              {renderTrafficTables(mdFlows, mdPacketRows)}
            </>
          )}

          {page === "analysis" && (
            <>
              <div className={styles.sectionHeader}>
                <h2 className={styles.sectionTitle}>{t("trdp.nav.analysis")}</h2>
              </div>
              {renderTrafficTables(flows, packetRows)}
            </>
          )}
        </section>
      </div>
    </div>
  );
}
