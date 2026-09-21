import { useEffect, useMemo, useRef } from "react";
import { useTranslation } from "react-i18next";
import GlassButton from "../../components/common/GlassButton";
import type { SystemViewEvent, SystemViewSnapshot } from "./model";
import type { RttSystemViewChannelState } from "./runtime-store";
import styles from "./SystemViewTraceView.module.css";

const MAX_RENDER_EVENTS = 4000;
const MAX_EVENT_ROWS = 800;
const MAX_TASK_LANES = 32;

interface Props {
  state: RttSystemViewChannelState | undefined;
  mode: "trace" | "events";
  controlAvailable: boolean;
  onStart: () => void;
  onStop: () => void;
  onRefreshTasks: () => void;
  onClear: () => void;
}

function formatFrequency(value: number | null | undefined): string {
  if (!value) return "—";
  if (value >= 1000000) return (value / 1000000).toFixed(value % 1000000 === 0 ? 0 : 1) + " MHz";
  if (value >= 1000) return (value / 1000).toFixed(1) + " kHz";
  return String(value) + " Hz";
}

function formatCycles(value: number): string {
  if (value >= 1000000000) return (value / 1000000000).toFixed(2) + "G";
  if (value >= 1000000) return (value / 1000000).toFixed(2) + "M";
  if (value >= 1000) return (value / 1000).toFixed(1) + "k";
  return String(value);
}

function eventLabel(event: SystemViewEvent): string {
  const detail = event.text
    ?? (event.context_id != null ? "0x" + event.context_id.toString(16) : null)
    ?? (event.value != null ? String(event.value) : null);
  return detail ? event.kind + " · " + detail : event.kind;
}

interface TimelineLabels {
  waiting: string;
  targetTime: string;
  isr: string;
  idle: string;
  unknownTask: string;
}

function drawTimeline(
  canvas: HTMLCanvasElement,
  events: readonly SystemViewEvent[],
  snapshot: SystemViewSnapshot | null,
  labels: TimelineLabels,
  cssWidth: number,
  cssHeight: number,
  dpr: number,
): void {
  const ctx = canvas.getContext("2d");
  if (!ctx) return;
  const style = getComputedStyle(canvas);
  const accent = style.getPropertyValue("--accent-primary").trim() || "#7aa2ff";
  const textPrimary = style.getPropertyValue("--text-primary").trim() || "#ffffff";
  const textMuted = style.getPropertyValue("--text-muted").trim() || "#9aa4b2";
  const divider = style.getPropertyValue("--content-divider").trim() || "rgba(255,255,255,.14)";
  const warning = style.getPropertyValue("--color-warning").trim() || "#f0b44d";
  const fontFamily = style.getPropertyValue("--font-ui").trim() || "sans-serif";
  const fontSize = style.getPropertyValue("--text-xs").trim() || "12px";

  ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
  ctx.clearRect(0, 0, cssWidth, cssHeight);

  const visible = events.slice(-MAX_RENDER_EVENTS);
  if (visible.length === 0) {
    ctx.fillStyle = textMuted;
    ctx.font = `${fontSize} ${fontFamily}`;
    ctx.fillText(labels.waiting, 24, 36);
    return;
  }

  const tasks = [...(snapshot?.tasks ?? [])]
    .sort((left, right) => left.id - right.id)
    .slice(0, MAX_TASK_LANES);
  const taskIds = new Set(tasks.map(task => task.id));
  const laneFor = new Map<number, number>();
  tasks.forEach((task, index) => laneFor.set(task.id, index));

  const left = 170;
  const right = Math.max(left + 120, cssWidth - 24);
  const top = 34;
  const laneHeight = 26;
  const startCycles = visible[0]?.target_cycles ?? 0;
  const endCycles = Math.max(startCycles + 1, visible[visible.length - 1]?.target_cycles ?? startCycles + 1);
  const xFor = (cycles: number) => left + ((cycles - startCycles) / (endCycles - startCycles)) * (right - left);

  ctx.strokeStyle = divider;
  ctx.lineWidth = 1;
  ctx.fillStyle = textMuted;
  ctx.font = `${fontSize} ${fontFamily}`;
  ctx.fillText(labels.targetTime, left, 20);

  for (let index = 0; index < tasks.length; index += 1) {
    const y = top + index * laneHeight;
    ctx.beginPath();
    ctx.moveTo(left, y + laneHeight);
    ctx.lineTo(right, y + laneHeight);
    ctx.stroke();
    ctx.fillStyle = textPrimary;
    const task = tasks[index];
    const name = task.name || `${labels.unknownTask} 0x${task.id.toString(16)}`;
    ctx.fillText(name, 12, y + laneHeight * 0.68);
  }

  const irqLane = tasks.length;
  const idleLane = tasks.length + 1;
  const irqY = top + irqLane * laneHeight;
  const idleY = top + idleLane * laneHeight;
  ctx.fillStyle = textPrimary;
  ctx.fillText(labels.isr, 12, irqY + laneHeight * 0.68);
  ctx.fillText(labels.idle, 12, idleY + laneHeight * 0.68);

  let activeTaskId: number | null = null;
  let activeTaskStart = 0;
  let interruptedTaskId: number | null = null;
  const taskIntervals: Array<{ id: number; start: number; end: number }> = [];
  let irqDepth = 0;
  let irqStart: number | null = null;
  const irqIntervals: Array<{ start: number; end: number }> = [];
  let idleStart: number | null = null;
  const idleIntervals: Array<{ start: number; end: number }> = [];
  const gapIntervals: Array<{ start: number; end: number }> = [];

  const closeTask = (end: number) => {
    if (activeTaskId == null) return;
    if (taskIds.has(activeTaskId)) {
      taskIntervals.push({ id: activeTaskId, start: activeTaskStart, end });
    }
    activeTaskId = null;
  };
  const closeIdle = (end: number) => {
    if (idleStart == null) return;
    idleIntervals.push({ start: idleStart, end });
    idleStart = null;
  };

  for (const event of visible) {
    if (event.event_id === 1) {
      const gapStart = Math.max(startCycles, event.target_cycles - event.delta_cycles);
      closeTask(gapStart);
      closeIdle(gapStart);
      if (irqStart != null) {
        irqIntervals.push({ start: irqStart, end: gapStart });
        irqStart = null;
      }
      irqDepth = 0;
      interruptedTaskId = null;
      gapIntervals.push({ start: gapStart, end: event.target_cycles });
    } else if (event.event_id === 4) {
      closeTask(event.target_cycles);
      if (event.context_id != null) {
        activeTaskId = event.context_id;
        activeTaskStart = event.target_cycles;
      }
      closeIdle(event.target_cycles);
    } else if (event.event_id === 5 || event.event_id === 11) {
      closeTask(event.target_cycles);
    } else if (event.event_id === 17) {
      closeTask(event.target_cycles);
      if (idleStart == null) idleStart = event.target_cycles;
    } else if (event.event_id === 2) {
      if (irqDepth === 0) {
        irqStart = event.target_cycles;
        interruptedTaskId = activeTaskId;
        closeTask(event.target_cycles);
      }
      irqDepth += 1;
    } else if (event.event_id === 3) {
      irqDepth = Math.max(0, irqDepth - 1);
      if (irqDepth === 0) {
        if (irqStart != null) {
          irqIntervals.push({ start: irqStart, end: event.target_cycles });
          irqStart = null;
        }
        if (interruptedTaskId != null) {
          activeTaskId = interruptedTaskId;
          activeTaskStart = event.target_cycles;
          interruptedTaskId = null;
        }
      }
    } else if (event.event_id === 18) {
      if (irqStart != null) {
        irqIntervals.push({ start: irqStart, end: event.target_cycles });
        irqStart = null;
      }
      irqDepth = 0;
      interruptedTaskId = null;
    }
  }
  closeTask(endCycles);
  closeIdle(endCycles);
  if (irqStart != null) irqIntervals.push({ start: irqStart, end: endCycles });

  const drawInterval = (start: number, end: number, lane: number, color: string, alpha: number) => {
    const x = xFor(start);
    const widthPx = Math.max(2, xFor(Math.max(start + 1, end)) - x);
    ctx.globalAlpha = alpha;
    ctx.fillStyle = color;
    ctx.fillRect(x, top + lane * laneHeight + 4, widthPx, Math.max(4, laneHeight - 8));
    ctx.globalAlpha = 1;
  };

  gapIntervals.forEach(interval => {
    const x = xFor(interval.start);
    const widthPx = Math.max(2, xFor(Math.max(interval.start + 1, interval.end)) - x);
    ctx.globalAlpha = 0.14;
    ctx.fillStyle = warning;
    ctx.fillRect(x, top, widthPx, (tasks.length + 2) * laneHeight);
    ctx.globalAlpha = 1;
  });
  taskIntervals.forEach(interval => {
    const lane = laneFor.get(interval.id);
    if (lane != null) drawInterval(interval.start, interval.end, lane, accent, 0.68);
  });
  irqIntervals.forEach(interval => drawInterval(interval.start, interval.end, irqLane, warning, 0.88));
  idleIntervals.forEach(interval => drawInterval(interval.start, interval.end, idleLane, textMuted, 0.42));

  const span = endCycles - startCycles;
  ctx.fillStyle = textMuted;
  ctx.font = `${fontSize} ${fontFamily}`;
  for (let tick = 0; tick <= 4; tick += 1) {
    const cycles = Math.round(startCycles + span * (tick / 4));
    const x = xFor(cycles);
    ctx.fillText(formatCycles(cycles), x - 18, cssHeight - 7);
  }
}

export default function SystemViewTraceView({
  state,
  mode,
  controlAvailable,
  onStart,
  onStop,
  onRefreshTasks,
  onClear,
}: Props) {
  const { t } = useTranslation();
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const metadataRefreshKeyRef = useRef("");
  const onRefreshTasksRef = useRef(onRefreshTasks);
  onRefreshTasksRef.current = onRefreshTasks;
  const snapshot = state?.snapshot ?? null;
  const events = state?.events ?? [];
  const unknownTasks = useMemo(
    () => (snapshot?.tasks ?? []).filter(task => !task.name || task.priority == null),
    [snapshot?.tasks],
  );
  const unknownTaskKey = useMemo(
    () => unknownTasks.map(task => task.id).sort((a, b) => a - b).join(","),
    [unknownTasks],
  );
  const integrityCompromised = Boolean(snapshot && (
    snapshot.target_overflow_packets > 0
    || snapshot.decoder_dropped_chunks > 0
    || snapshot.decoder_errors > 0
    || snapshot.presentation_dropped_events > 0
  ));

  useEffect(() => {
    if (!controlAvailable || !unknownTaskKey || !snapshot) return;
    const key = `${snapshot.generation}:${unknownTaskKey}`;
    if (metadataRefreshKeyRef.current === key) return;
    metadataRefreshKeyRef.current = key;

    // Metadata can arrive after the task's first execution event. Retry a small bounded sequence
    // instead of polling forever; any resolved/changed unknown-task set cancels the old sequence.
    const timers = [350, 2500, 10000].map(delay => window.setTimeout(
      () => onRefreshTasksRef.current(),
      delay,
    ));
    return () => timers.forEach(timer => window.clearTimeout(timer));
  }, [controlAvailable, snapshot?.generation, unknownTaskKey]);

  useEffect(() => {
    if (mode !== "trace" || !canvasRef.current) return;
    const canvas = canvasRef.current;
    const scroller = canvas.parentElement;
    if (!scroller) return;

    const render = () => {
      const laneCount = Math.min(snapshot?.tasks.length ?? 0, MAX_TASK_LANES) + 2;
      const cssWidth = Math.max(760, scroller.clientWidth);
      const cssHeight = Math.max(220, 68 + laneCount * 26);
      const dpr = Math.max(1, window.devicePixelRatio || 1);
      const pixelWidth = Math.round(cssWidth * dpr);
      const pixelHeight = Math.round(cssHeight * dpr);
      if (canvas.width !== pixelWidth) canvas.width = pixelWidth;
      if (canvas.height !== pixelHeight) canvas.height = pixelHeight;
      canvas.style.width = `${cssWidth}px`;
      canvas.style.height = `${cssHeight}px`;
      drawTimeline(canvas, events, snapshot, {
        waiting: t("rtt.traceWaiting"),
        targetTime: t("rtt.traceTargetTime"),
        isr: t("rtt.traceIsr"),
        idle: t("rtt.traceIdleLane"),
        unknownTask: t("rtt.traceUnknownTask"),
      }, cssWidth, cssHeight, dpr);
    };

    render();
    const observer = new ResizeObserver(render);
    observer.observe(scroller);
    return () => observer.disconnect();
  }, [events, mode, snapshot, t]);

  const visibleEvents = useMemo(() => events.slice(-MAX_EVENT_ROWS).reverse(), [events]);
  const windowCycles = useMemo(() => {
    if (!snapshot) return 0;
    return Math.max(0, snapshot.last_target_cycles - snapshot.window_start_cycles);
  }, [snapshot]);

  return (
    <div className={styles.root}>
      <div className={styles.traceToolbar}>
        <div className={styles.traceState}>
          <span
            className={styles.stateDot + (snapshot?.phase === "recording" ? " " + styles.recording : "")}
            aria-hidden="true"
          />
          <strong>
            {snapshot?.phase === "recording"
              ? t("rtt.traceRecording")
              : snapshot?.phase === "stopped"
                ? t("rtt.traceStopped")
                : t("rtt.traceIdle")}
          </strong>
        </div>
        <div className={styles.actions}>
          {snapshot?.phase === "recording" ? (
            <GlassButton size="sm" disabled={!controlAvailable} onClick={onStop}>
              {t("rtt.traceStop")}
            </GlassButton>
          ) : (
            <GlassButton size="sm" disabled={!controlAvailable} onClick={onStart}>
              {t("rtt.traceStart")}
            </GlassButton>
          )}
          <GlassButton size="sm" onClick={onClear}>{t("rtt.traceClear")}</GlassButton>
        </div>
      </div>

      {state?.error && <div className={styles.error}>{state.error}</div>}
      {integrityCompromised && (
        <div className={styles.integrityWarning}>{t("rtt.traceIncomplete")}</div>
      )}

      <div className={styles.metrics}>
        <div className={styles.metric}><span>{t("rtt.traceEvents")}</span><strong>{snapshot?.event_count ?? 0}</strong></div>
        <div className={styles.metric}><span>{t("rtt.traceTasks")}</span><strong>{snapshot?.task_count ?? 0}</strong></div>
        <div className={styles.metric}><span>{t("rtt.traceTargetOverflow")}</span><strong>{snapshot?.target_dropped_events ?? 0}</strong></div>
        <div className={styles.metric}><span>{t("rtt.traceDecoderLoss")}</span><strong>{snapshot?.decoder_dropped_chunks ?? 0}</strong></div>
        <div className={styles.metric}><span>{t("rtt.traceClock")}</span><strong>{formatFrequency(snapshot?.cpu_freq_hz ?? snapshot?.sys_freq_hz)}</strong></div>
      </div>

      {mode === "trace" ? (
        <div className={styles.traceBody}>
          <section className={styles.timelineSection}>
            <div className={styles.sectionHeader}>
              <strong>{t("rtt.traceTimeline")}</strong>
              <span>
                {(snapshot?.task_count ?? 0) > MAX_TASK_LANES
                  ? t("rtt.traceLaneLimit", { visible: MAX_TASK_LANES, total: snapshot?.task_count ?? 0 })
                  : t("rtt.traceTargetTime")}
              </span>
            </div>
            <div className={styles.canvasScroller}>
              <canvas ref={canvasRef} className={styles.timeline} />
            </div>
          </section>

          <section className={styles.tasksSection}>
            <div className={styles.sectionHeader}>
              <strong>{t("rtt.traceTaskStats")}</strong>
              <span>
                {unknownTasks.length > 0
                  ? t("rtt.traceMetadataSyncing", { count: unknownTasks.length })
                  : snapshot?.system_description?.[0] ?? ""}
              </span>
            </div>
            <div className={styles.taskTable}>
              <div className={`${styles.taskRow} ${styles.taskHeader} liquid-control-surface`}>
                <span>{t("rtt.traceTask")}</span>
                <span>{t("rtt.tracePriority")}</span>
                <span>{t("rtt.traceSwitches")}</span>
                <span>{t("rtt.traceRuntime")}</span>
              </div>
              {(snapshot?.tasks ?? []).map(task => {
                const percent = windowCycles > 0 ? (task.runtime_cycles / windowCycles) * 100 : 0;
                return (
                  <div key={task.id} className={styles.taskRow}>
                    <span>{task.name || `${t("rtt.traceUnknownTask")} 0x${task.id.toString(16)}`}</span>
                    <span>{task.priority ?? "—"}</span>
                    <span>{task.switches}</span>
                    <span>{formatCycles(task.runtime_cycles)} · {percent.toFixed(1)}%</span>
                  </div>
                );
              })}
              {(snapshot?.tasks.length ?? 0) === 0 && <div className={styles.empty}>{t("rtt.traceWaiting")}</div>}
            </div>
          </section>
        </div>
      ) : (
        <div className={styles.eventList}>
          {visibleEvents.length === 0 ? (
            <div className={styles.empty}>{t("rtt.traceWaiting")}</div>
          ) : visibleEvents.map(event => (
            <div key={event.sequence} className={styles.eventRow}>
              <span className={styles.eventMeta}>#{event.sequence} · {formatCycles(event.target_cycles)}</span>
              <span className={styles.eventName}>{eventLabel(event)}</span>
              <span className={styles.eventDelta}>+{formatCycles(event.delta_cycles)}</span>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
