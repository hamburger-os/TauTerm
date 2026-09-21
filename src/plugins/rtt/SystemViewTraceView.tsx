import { useEffect, useMemo, useRef } from "react";
import { useTranslation } from "react-i18next";
import type { SystemViewEvent, SystemViewSnapshot } from "./model";
import type { RttSystemViewChannelState } from "./runtime-store";
import styles from "./SystemViewTraceView.module.css";

const MAX_RENDER_EVENTS = 4000;
const MAX_EVENT_ROWS = 800;
const MAX_TASK_LANES = 10;

interface Props {
  state: RttSystemViewChannelState | undefined;
  mode: "trace" | "events";
  onStart: () => void;
  onStop: () => void;
  onRefresh: () => void;
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

function drawTimeline(
  canvas: HTMLCanvasElement,
  events: readonly SystemViewEvent[],
  snapshot: SystemViewSnapshot | null,
): void {
  const ctx = canvas.getContext("2d");
  if (!ctx) return;
  const style = getComputedStyle(canvas);
  const accent = style.getPropertyValue("--accent-primary").trim() || "#7aa2ff";
  const textPrimary = style.getPropertyValue("--text-primary").trim() || "#ffffff";
  const textMuted = style.getPropertyValue("--text-muted").trim() || "#9aa4b2";
  const divider = style.getPropertyValue("--content-divider").trim() || "rgba(255,255,255,.14)";
  const warning = style.getPropertyValue("--color-warning").trim() || "#f0b44d";
  const width = canvas.width;
  const height = canvas.height;
  ctx.clearRect(0, 0, width, height);

  const visible = events.slice(-MAX_RENDER_EVENTS);
  if (visible.length === 0) {
    ctx.fillStyle = textMuted;
    ctx.font = "24px sans-serif";
    ctx.fillText("Waiting for SystemView events...", 36, 56);
    return;
  }

  const tasks = (snapshot?.tasks ?? []).slice(0, MAX_TASK_LANES);
  const taskIds = new Set(tasks.map(task => task.id));
  const laneFor = new Map<number, number>();
  tasks.forEach((task, index) => laneFor.set(task.id, index));

  const left = 170;
  const right = width - 24;
  const top = 42;
  const laneHeight = Math.max(22, Math.floor((height - top - 28) / Math.max(tasks.length + 2, 3)));
  const startCycles = visible[0]?.target_cycles ?? 0;
  const endCycles = Math.max(startCycles + 1, visible[visible.length - 1]?.target_cycles ?? startCycles + 1);
  const xFor = (cycles: number) => left + ((cycles - startCycles) / (endCycles - startCycles)) * (right - left);

  ctx.strokeStyle = divider;
  ctx.lineWidth = 1;
  ctx.fillStyle = textMuted;
  ctx.font = "18px sans-serif";
  ctx.fillText("Target time", left, 24);

  for (let index = 0; index < tasks.length; index += 1) {
    const y = top + index * laneHeight;
    ctx.beginPath();
    ctx.moveTo(left, y + laneHeight);
    ctx.lineTo(right, y + laneHeight);
    ctx.stroke();
    ctx.fillStyle = textPrimary;
    const task = tasks[index];
    ctx.fillText(task.name || "Task 0x" + task.id.toString(16), 12, y + laneHeight * 0.68);
  }

  const irqLane = tasks.length;
  const idleLane = tasks.length + 1;
  const irqY = top + irqLane * laneHeight;
  const idleY = top + idleLane * laneHeight;
  ctx.fillStyle = textPrimary;
  ctx.fillText("ISR", 12, irqY + laneHeight * 0.68);
  ctx.fillText("Idle", 12, idleY + laneHeight * 0.68);

  let activeTask: { id: number; start: number } | null = null;
  const taskIntervals: Array<{ id: number; start: number; end: number }> = [];
  let irqDepth = 0;
  let irqStart: number | null = null;
  const irqIntervals: Array<{ start: number; end: number }> = [];
  let idleStart: number | null = null;
  const idleIntervals: Array<{ start: number; end: number }> = [];

  const closeTask = (end: number) => {
    if (!activeTask) return;
    if (taskIds.has(activeTask.id)) taskIntervals.push({ id: activeTask.id, start: activeTask.start, end });
    activeTask = null;
  };

  for (const event of visible) {
    if (event.event_id === 4) {
      closeTask(event.target_cycles);
      if (event.context_id != null) activeTask = { id: event.context_id, start: event.target_cycles };
      if (idleStart != null) {
        idleIntervals.push({ start: idleStart, end: event.target_cycles });
        idleStart = null;
      }
    } else if (event.event_id === 5 || event.event_id === 11) {
      closeTask(event.target_cycles);
    } else if (event.event_id === 17) {
      closeTask(event.target_cycles);
      if (idleStart == null) idleStart = event.target_cycles;
    } else if (event.event_id === 2) {
      if (irqDepth === 0) irqStart = event.target_cycles;
      irqDepth += 1;
    } else if (event.event_id === 3 || event.event_id === 18) {
      irqDepth = Math.max(0, irqDepth - 1);
      if (irqDepth === 0 && irqStart != null) {
        irqIntervals.push({ start: irqStart, end: event.target_cycles });
        irqStart = null;
      }
    }
  }
  closeTask(endCycles);
  if (irqStart != null) irqIntervals.push({ start: irqStart, end: endCycles });
  if (idleStart != null) idleIntervals.push({ start: idleStart, end: endCycles });

  const drawInterval = (start: number, end: number, lane: number, color: string, alpha: number) => {
    const x = xFor(start);
    const widthPx = Math.max(2, xFor(Math.max(start + 1, end)) - x);
    ctx.globalAlpha = alpha;
    ctx.fillStyle = color;
    ctx.fillRect(x, top + lane * laneHeight + 4, widthPx, Math.max(4, laneHeight - 8));
    ctx.globalAlpha = 1;
  };

  taskIntervals.forEach(interval => {
    const lane = laneFor.get(interval.id);
    if (lane != null) drawInterval(interval.start, interval.end, lane, accent, 0.68);
  });
  irqIntervals.forEach(interval => drawInterval(interval.start, interval.end, irqLane, warning, 0.88));
  idleIntervals.forEach(interval => drawInterval(interval.start, interval.end, idleLane, textMuted, 0.42));

  const span = endCycles - startCycles;
  ctx.fillStyle = textMuted;
  ctx.font = "16px sans-serif";
  for (let tick = 0; tick <= 4; tick += 1) {
    const cycles = Math.round(startCycles + span * (tick / 4));
    const x = xFor(cycles);
    ctx.fillText(formatCycles(cycles), x - 18, height - 7);
  }
}

export default function SystemViewTraceView({
  state,
  mode,
  onStart,
  onStop,
  onRefresh,
  onClear,
}: Props) {
  const { t } = useTranslation();
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const snapshot = state?.snapshot ?? null;
  const events = state?.events ?? [];

  useEffect(() => {
    if (mode !== "trace" || !canvasRef.current) return;
    drawTimeline(canvasRef.current, events, snapshot);
  }, [events, mode, snapshot]);

  const visibleEvents = useMemo(() => events.slice(-MAX_EVENT_ROWS).reverse(), [events]);
  const totalRuntime = useMemo(
    () => (snapshot?.tasks ?? []).reduce((sum, task) => sum + task.runtime_cycles, 0),
    [snapshot?.tasks],
  );

  return (
    <div className={styles.root}>
      <div className={styles.traceToolbar}>
        <div className={styles.traceState}>
          <span
            className={styles.stateDot + (snapshot?.phase === "recording" ? " " + styles.recording : "")}
            aria-hidden="true"
          />
          <strong>{snapshot?.phase === "recording" ? t("rtt.traceRecording") : t("rtt.traceStopped")}</strong>
        </div>
        <div className={styles.actions}>
          <button type="button" className="liquid-glass-button" disabled={!snapshot?.control_available || snapshot?.phase === "recording"} onClick={onStart}>
            {t("rtt.traceStart")}
          </button>
          <button type="button" className="liquid-glass-button" disabled={!snapshot?.control_available || snapshot?.phase !== "recording"} onClick={onStop}>
            {t("rtt.traceStop")}
          </button>
          <button type="button" className="liquid-glass-button" onClick={onRefresh}>{t("rtt.traceRefresh")}</button>
          <button type="button" className="liquid-glass-button" onClick={onClear}>{t("rtt.traceClear")}</button>
        </div>
      </div>

      {state?.error && <div className={styles.error}>{state.error}</div>}

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
              <span>{t("rtt.traceTargetTime")}</span>
            </div>
            <div className={styles.canvasScroller}>
              <canvas ref={canvasRef} width={1600} height={360} className={styles.timeline} />
            </div>
          </section>

          <section className={styles.tasksSection}>
            <div className={styles.sectionHeader}>
              <strong>{t("rtt.traceTaskStats")}</strong>
              <span>{snapshot?.system_description?.[0] ?? ""}</span>
            </div>
            <div className={styles.taskTable}>
              <div className={styles.taskRow + " " + styles.taskHeader}>
                <span>{t("rtt.traceTask")}</span>
                <span>{t("rtt.tracePriority")}</span>
                <span>{t("rtt.traceSwitches")}</span>
                <span>{t("rtt.traceRuntime")}</span>
              </div>
              {(snapshot?.tasks ?? []).slice(0, 24).map(task => {
                const percent = totalRuntime > 0 ? (task.runtime_cycles / totalRuntime) * 100 : 0;
                return (
                  <div key={task.id} className={styles.taskRow}>
                    <span>{task.name || "0x" + task.id.toString(16)}</span>
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
