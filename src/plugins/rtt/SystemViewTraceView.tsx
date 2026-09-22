import { useEffect, useMemo, useRef, useState } from "react";
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
  upBufferSize?: number | null;
  onStart: () => void;
  onStop: () => void;
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

function formatTargetTime(
  cycles: number,
  originCycles: number,
  sysFrequency: number | null | undefined,
): string {
  if (!sysFrequency) return formatCycles(cycles);
  const seconds = Math.max(0, cycles - originCycles) / sysFrequency;
  if (seconds >= 1) return `+${seconds.toFixed(seconds >= 10 ? 2 : 3)} s`;
  const milliseconds = seconds * 1_000;
  if (milliseconds >= 1) return `+${milliseconds.toFixed(milliseconds >= 10 ? 1 : 2)} ms`;
  const microseconds = seconds * 1_000_000;
  return `+${microseconds.toFixed(microseconds >= 10 ? 1 : 2)} µs`;
}

function formatTargetDuration(
  cycles: number,
  sysFrequency: number | null | undefined,
): string {
  if (!sysFrequency) return formatCycles(cycles);
  const seconds = cycles / sysFrequency;
  if (seconds >= 1) return `${seconds.toFixed(seconds >= 10 ? 2 : 3)} s`;
  const milliseconds = seconds * 1_000;
  if (milliseconds >= 1) return `${milliseconds.toFixed(milliseconds >= 10 ? 1 : 2)} ms`;
  const microseconds = seconds * 1_000_000;
  return `${microseconds.toFixed(microseconds >= 10 ? 1 : 2)} µs`;
}

function formatRate(value: number | null): string {
  if (value == null || !Number.isFinite(value)) return "—";
  if (value >= 1_000_000) return `${(value / 1_000_000).toFixed(1)}M/s`;
  if (value >= 1_000) return `${(value / 1_000).toFixed(1)}k/s`;
  return `${value.toFixed(value >= 10 ? 0 : 1)}/s`;
}

function formatTaskShare(percent: number, incomplete: boolean): string {
  if (percent <= 0) return "0%";
  const digits = percent < 0.01 ? 3 : percent < 0.1 ? 2 : 1;
  return `${incomplete ? "≥" : ""}${percent.toFixed(digits)}%`;
}

function formatSystemDescription(values: readonly string[]): string {
  const parts = values
    .flatMap(value => value.split(","))
    .map(part => part.trim().replace(/^[A-Za-z]=/, ""))
    .filter(Boolean);
  return parts.join(" · ");
}

function restoreTargetId(
  taskId: number,
  snapshot: SystemViewSnapshot | null,
): string | null {
  if (snapshot?.ram_base == null || snapshot.id_shift == null) return null;
  const restored = BigInt(snapshot.ram_base) + (BigInt(taskId) << BigInt(snapshot.id_shift));
  return `0x${restored.toString(16).toUpperCase()}`;
}

interface TimelineSeed {
  activeTaskId: number | null;
  interruptedTaskId: number | null;
  interruptedIdle: boolean;
  irqDepth: number;
  idleActive: boolean;
}

function timelineSeed(
  events: readonly SystemViewEvent[],
  initial: SystemViewSnapshot["history_entry_state"] | null | undefined,
): TimelineSeed {
  const seed: TimelineSeed = {
    activeTaskId: initial?.active_task_id ?? null,
    interruptedTaskId: initial?.interrupted_task_id ?? null,
    interruptedIdle: initial?.interrupted_idle ?? false,
    irqDepth: initial?.irq_depth ?? 0,
    idleActive: initial?.idle_active ?? false,
  };

  const reset = () => {
    seed.activeTaskId = null;
    seed.interruptedTaskId = null;
    seed.interruptedIdle = false;
    seed.irqDepth = 0;
    seed.idleActive = false;
  };

  for (const event of events) {
    if (event.sync_boundary) reset();
    switch (event.event_id) {
      case 1:
      case 10:
      case 11:
        reset();
        break;
      case 2:
        if (seed.irqDepth === 0) {
          seed.interruptedTaskId = seed.activeTaskId;
          seed.interruptedIdle = seed.idleActive;
          seed.activeTaskId = null;
          seed.idleActive = false;
        }
        seed.irqDepth += 1;
        break;
      case 3:
        seed.irqDepth = Math.max(0, seed.irqDepth - 1);
        if (seed.irqDepth === 0) {
          seed.activeTaskId = seed.interruptedTaskId;
          seed.idleActive = seed.interruptedIdle;
          seed.interruptedTaskId = null;
          seed.interruptedIdle = false;
        }
        break;
      case 4:
        seed.activeTaskId = event.context_id ?? null;
        seed.interruptedTaskId = null;
        seed.interruptedIdle = false;
        seed.irqDepth = 0;
        seed.idleActive = false;
        break;
      case 5:
        seed.activeTaskId = null;
        break;
      case 17:
        seed.activeTaskId = null;
        seed.idleActive = true;
        break;
      case 18:
        reset();
        break;
      case 29:
        if (event.context_id != null && seed.activeTaskId === event.context_id) {
          seed.activeTaskId = null;
        }
        break;
      default:
        break;
    }
  }
  return seed;
}

function selectTimelineTasks(
  snapshot: SystemViewSnapshot | null,
  visibleEvents: readonly SystemViewEvent[],
  seed: TimelineSeed,
): SystemViewSnapshot["tasks"] {
  const tasks = snapshot?.tasks ?? [];
  if (tasks.length <= MAX_TASK_LANES) return [...tasks].sort((left, right) => left.id - right.id);

  const lastExecution = new Map<number, number>();
  visibleEvents.forEach((event, index) => {
    if (event.event_id === 4 && event.context_id != null) {
      lastExecution.set(event.context_id, index);
    }
  });
  const pinned = new Set<number>();
  if (seed.activeTaskId != null) pinned.add(seed.activeTaskId);
  if (seed.interruptedTaskId != null) pinned.add(seed.interruptedTaskId);

  const selected = [...tasks]
    .sort((left, right) => {
      const leftPinned = pinned.has(left.id) ? 1 : 0;
      const rightPinned = pinned.has(right.id) ? 1 : 0;
      if (leftPinned !== rightPinned) return rightPinned - leftPinned;

      const leftRecent = lastExecution.get(left.id) ?? -1;
      const rightRecent = lastExecution.get(right.id) ?? -1;
      const leftVisible = leftRecent >= 0 ? 1 : 0;
      const rightVisible = rightRecent >= 0 ? 1 : 0;
      if (leftVisible !== rightVisible) return rightVisible - leftVisible;
      if (leftRecent !== rightRecent) return rightRecent - leftRecent;

      return right.runtime_cycles - left.runtime_cycles || left.id - right.id;
    })
    .slice(0, MAX_TASK_LANES);

  return selected.sort((left, right) => left.id - right.id);
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

  const visibleStart = Math.max(0, events.length - MAX_RENDER_EVENTS);
  const visible = events.slice(visibleStart);
  if (visible.length === 0) {
    ctx.fillStyle = textMuted;
    ctx.font = `${fontSize} ${fontFamily}`;
    ctx.fillText(labels.waiting, 24, 36);
    return;
  }

  const seed = timelineSeed(
    events.slice(0, visibleStart),
    snapshot?.history_entry_state,
  );
  const tasks = selectTimelineTasks(snapshot, visible, seed);
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

  let activeTaskId: number | null = seed.activeTaskId;
  let activeTaskStart = startCycles;
  let interruptedTaskId: number | null = seed.interruptedTaskId;
  let interruptedIdle = seed.interruptedIdle;
  const taskIntervals: Array<{ id: number; start: number; end: number }> = [];
  let irqDepth = seed.irqDepth;
  let irqStart: number | null = seed.irqDepth > 0 ? startCycles : null;
  const irqIntervals: Array<{ start: number; end: number }> = [];
  let idleStart: number | null = seed.idleActive ? startCycles : null;
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
      interruptedIdle = false;
      gapIntervals.push({ start: gapStart, end: event.target_cycles });
    } else if (event.event_id === 10) {
      closeTask(event.target_cycles);
      closeIdle(event.target_cycles);
      if (irqStart != null) {
        irqIntervals.push({ start: irqStart, end: event.target_cycles });
        irqStart = null;
      }
      irqDepth = 0;
      interruptedTaskId = null;
      interruptedIdle = false;
    } else if (event.event_id === 4) {
      closeTask(event.target_cycles);
      closeIdle(event.target_cycles);
      if (irqStart != null) {
        irqIntervals.push({ start: irqStart, end: event.target_cycles });
        irqStart = null;
      }
      irqDepth = 0;
      interruptedTaskId = null;
      interruptedIdle = false;
      if (event.context_id != null) {
        activeTaskId = event.context_id;
        activeTaskStart = event.target_cycles;
      }
    } else if (event.event_id === 5) {
      closeTask(event.target_cycles);
    } else if (event.event_id === 11) {
      closeTask(event.target_cycles);
      closeIdle(event.target_cycles);
      if (irqStart != null) {
        irqIntervals.push({ start: irqStart, end: event.target_cycles });
        irqStart = null;
      }
      irqDepth = 0;
      interruptedTaskId = null;
      interruptedIdle = false;
    } else if (event.event_id === 17) {
      closeTask(event.target_cycles);
      if (idleStart == null) idleStart = event.target_cycles;
    } else if (event.event_id === 2) {
      if (irqDepth === 0) {
        irqStart = event.target_cycles;
        interruptedTaskId = activeTaskId;
        interruptedIdle = idleStart != null;
        closeTask(event.target_cycles);
        closeIdle(event.target_cycles);
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
        } else if (interruptedIdle) {
          idleStart = event.target_cycles;
        }
        interruptedTaskId = null;
        interruptedIdle = false;
      }
    } else if (event.event_id === 18) {
      if (irqStart != null) {
        irqIntervals.push({ start: irqStart, end: event.target_cycles });
        irqStart = null;
      }
      irqDepth = 0;
      interruptedTaskId = null;
      interruptedIdle = false;
    } else if (
      event.event_id === 29
      && event.context_id != null
      && activeTaskId === event.context_id
    ) {
      closeTask(event.target_cycles);
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
    const label = formatTargetTime(cycles, startCycles, snapshot?.sys_freq_hz);
    const labelWidth = ctx.measureText(label).width;
    const labelX = Math.min(
      Math.max(4, x - labelWidth / 2),
      Math.max(4, cssWidth - labelWidth - 4),
    );
    ctx.fillText(label, labelX, cssHeight - 7);
  }
}

export default function SystemViewTraceView({
  state,
  mode,
  controlAvailable,
  upBufferSize,
  onStart,
  onStop,
  onClear,
}: Props) {
  const { t } = useTranslation();
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const snapshot = state?.snapshot ?? null;
  const events = state?.events ?? [];
  const metadataSync = state?.metadataSync;
  const presentationPending = state?.presentationRecovery.pending_drops
    ?? snapshot?.presentation_dropped_events
    ?? 0;
  const presentationRecovered = Math.max(
    0,
    (snapshot?.presentation_dropped_events ?? 0) - presentationPending,
  );
  const accountedEvents = (snapshot?.event_count ?? 0) + (snapshot?.target_dropped_events ?? 0);
  const observedCoverage = accountedEvents > 0
    ? (snapshot?.event_count ?? 0) / accountedEvents
    : null;
  const severeTargetLoss = observedCoverage != null
    && snapshot != null
    && snapshot.target_dropped_events > 0
    && observedCoverage < 0.5;
  const integrityCompromised = Boolean(snapshot && (
    snapshot.target_overflow_packets > 0
    || snapshot.decoder_dropped_chunks > 0
    || snapshot.decoder_errors > 0
    || presentationPending > 0
  ));
  const taskStatsIncomplete = Boolean(snapshot && (
    snapshot.target_overflow_packets > 0
    || snapshot.target_dropped_events > 0
    || snapshot.decoder_dropped_chunks > 0
    || snapshot.decoder_errors > 0
  ));
  const rateSampleRef = useRef<{
    generation: number;
    at: number;
    events: number;
    targetDrops: number;
  } | null>(null);
  const [traceRates, setTraceRates] = useState<{
    eventsPerSecond: number | null;
    targetDropsPerSecond: number | null;
  }>({ eventsPerSecond: null, targetDropsPerSecond: null });
  const metadataStatus = useMemo(() => {
    if (!metadataSync || metadataSync.unresolved_tasks === 0) {
      return formatSystemDescription(snapshot?.system_description ?? []);
    }
    if (metadataSync.phase === "unavailable") {
      return t("rtt.traceMetadataUnavailable", { count: metadataSync.unresolved_tasks });
    }
    if (metadataSync.phase === "incomplete") {
      return t("rtt.traceMetadataIncomplete", {
        count: metadataSync.unresolved_tasks,
        attempts: metadataSync.attempts,
      });
    }
    return t("rtt.traceMetadataSyncing", {
      count: metadataSync.unresolved_tasks,
      attempts: metadataSync.attempts,
      max: metadataSync.max_attempts,
    });
  }, [metadataSync, snapshot?.system_description, t]);

  useEffect(() => {
    if (!snapshot) {
      rateSampleRef.current = null;
      setTraceRates({ eventsPerSecond: null, targetDropsPerSecond: null });
      return;
    }
    const now = performance.now();
    const previous = rateSampleRef.current;
    if (
      !previous
      || previous.generation !== snapshot.generation
      || snapshot.event_count < previous.events
      || snapshot.target_dropped_events < previous.targetDrops
    ) {
      rateSampleRef.current = {
        generation: snapshot.generation,
        at: now,
        events: snapshot.event_count,
        targetDrops: snapshot.target_dropped_events,
      };
      setTraceRates({ eventsPerSecond: null, targetDropsPerSecond: null });
      return;
    }

    const elapsedSeconds = (now - previous.at) / 1_000;
    if (elapsedSeconds < 1) return;
    setTraceRates({
      eventsPerSecond: (snapshot.event_count - previous.events) / elapsedSeconds,
      targetDropsPerSecond: (snapshot.target_dropped_events - previous.targetDrops) / elapsedSeconds,
    });
    rateSampleRef.current = {
      generation: snapshot.generation,
      at: now,
      events: snapshot.event_count,
      targetDrops: snapshot.target_dropped_events,
    };
  }, [snapshot?.event_count, snapshot?.generation, snapshot?.target_dropped_events]);

  const integrityDetails = useMemo(() => {
    if (!snapshot) return "";
    const details: string[] = [];
    if (snapshot.target_dropped_events > 0) {
      details.push(t("rtt.traceTargetLossDiagnostic", {
        target: snapshot.target_dropped_events.toLocaleString(),
        rate: formatRate(snapshot.phase === "recording" ? traceRates.targetDropsPerSecond : null),
        observedRate: formatRate(snapshot.phase === "recording" ? traceRates.eventsPerSecond : null),
        coverage: observedCoverage == null ? "—" : `${(observedCoverage * 100).toFixed(1)}%`,
      }));
    }
    if (snapshot.decoder_dropped_chunks > 0 || snapshot.decoder_errors > 0) {
      details.push(t("rtt.traceDecoderDiagnostic", {
        queue: snapshot.decoder_dropped_chunks,
        errors: snapshot.decoder_errors,
      }));
    }
    if (presentationPending > 0) {
      details.push(t("rtt.tracePresentationPending", { count: presentationPending }));
    } else if (presentationRecovered > 0) {
      details.push(t("rtt.tracePresentationRecovered", { count: presentationRecovered }));
    }
    return details.join(" · ");
  }, [
    observedCoverage,
    presentationPending,
    presentationRecovered,
    snapshot,
    t,
    traceRates.eventsPerSecond,
    traceRates.targetDropsPerSecond,
  ]);

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
  const eventTimeOrigin = events[0]?.target_cycles ?? 0;
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
        <div className={styles.integrityWarning}>
          <span>
            {severeTargetLoss ? t("rtt.traceSevereTargetLoss") : t("rtt.traceIncomplete")}
          </span>
          <span className={styles.integrityDetails}>{integrityDetails}</span>
        </div>
      )}

      <div className={styles.metrics}>
        <div className={styles.metric}><span>{t("rtt.traceEvents")}</span><strong>{(snapshot?.event_count ?? 0).toLocaleString()}</strong></div>
        <div className={styles.metric}><span>{t("rtt.traceTasks")}</span><strong>{snapshot?.task_count ?? 0}</strong></div>
        <div
          className={styles.metric}
          title={t("rtt.traceTargetOverflowHint", {
            packets: snapshot?.target_overflow_packets ?? 0,
            buffer: upBufferSize ?? "—",
            rate: formatRate(snapshot?.phase === "recording" ? traceRates.targetDropsPerSecond : null),
          })}
        >
          <span>{t("rtt.traceTargetOverflow")}</span><strong>{(snapshot?.target_dropped_events ?? 0).toLocaleString()}</strong>
        </div>
        <div className={styles.metric}><span>{t("rtt.traceDecoderLoss")}</span><strong>{(snapshot?.decoder_dropped_chunks ?? 0).toLocaleString()}</strong></div>
        <div className={styles.metric}><span>{t("rtt.traceTimestampClock")}</span><strong>{formatFrequency(snapshot?.sys_freq_hz)}</strong></div>
        <div className={styles.metric}><span>{t("rtt.traceClock")}</span><strong>{formatFrequency(snapshot?.cpu_freq_hz)}</strong></div>
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
              <span title={(snapshot?.system_description ?? []).join("\n") || metadataStatus}>
                {metadataStatus}
              </span>
            </div>
            <div className={styles.taskTable}>
              <div className={`${styles.taskRow} ${styles.taskHeader} liquid-glass-content-header`}>
                <span>{t("rtt.traceTask")}</span>
                <span>{t("rtt.tracePriority")}</span>
                <span>{t("rtt.traceSwitches")}</span>
                <span>{t("rtt.traceRuntime")}</span>
              </div>
              <div className={styles.taskRows}>
                {(snapshot?.tasks ?? []).map(task => {
                  const percent = windowCycles > 0 ? (task.runtime_cycles / windowCycles) * 100 : 0;
                  const runtime = formatTargetDuration(task.runtime_cycles, snapshot?.sys_freq_hz);
                  const share = severeTargetLoss
                    ? t("rtt.traceRuntimeShareUnavailable")
                    : formatTaskShare(percent, taskStatsIncomplete);
                  return (
                    <div key={task.id} className={styles.taskRow}>
                      <span
                        title={(() => {
                          const compressed = `0x${task.id.toString(16).toUpperCase()}`;
                          const restored = restoreTargetId(task.id, snapshot);
                          const identity = restored
                            ? t("rtt.traceTaskIdDetails", { compressed, restored })
                            : t("rtt.traceTaskIdOnly", { compressed });
                          return task.name ? `${task.name} · ${identity}` : identity;
                        })()}
                      >
                        {task.name || `${t("rtt.traceUnknownTask")} · ID 0x${task.id.toString(16).toUpperCase()}`}
                      </span>
                      <span>{task.priority ?? "—"}</span>
                      <span>{task.switches}</span>
                      <span title={severeTargetLoss ? t("rtt.traceRuntimeShareUnavailableHint") : undefined}>
                        {runtime} · {share}
                      </span>
                    </div>
                  );
                })}
                {(snapshot?.tasks.length ?? 0) === 0 && <div className={styles.empty}>{t("rtt.traceWaiting")}</div>}
              </div>
            </div>
          </section>
        </div>
      ) : (
        <div className={styles.eventList}>
          {visibleEvents.length === 0 ? (
            <div className={styles.empty}>{t("rtt.traceWaiting")}</div>
          ) : visibleEvents.map(event => (
            <div key={event.sequence} className={styles.eventRow}>
              <span
                className={styles.eventMeta}
                title={`${event.target_cycles} cycles`}
              >
                #{event.sequence} · {formatTargetTime(event.target_cycles, eventTimeOrigin, snapshot?.sys_freq_hz)}
              </span>
              <span className={styles.eventName}>{eventLabel(event)}</span>
              <span
                className={styles.eventDelta}
                title={`+${event.delta_cycles} cycles`}
              >
                +{formatTargetDuration(event.delta_cycles, snapshot?.sys_freq_hz)}
              </span>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
