import type { Terminal } from "@xterm/xterm";
import type { FitAddon } from "@xterm/addon-fit";

export interface XtermHostInstance {
  terminal: Terminal;
  fitAddon: FitAddon;
  onOpened?: () => void;
  onDispose?: () => void;
}

export interface XtermHostLifecycle {
  readonly terminal: Terminal | null;
  scheduleFit(): void;
  dispose(): void;
}

interface XtermHostLifecycleOptions {
  create: () => XtermHostInstance;
  onFit?: (terminal: Terminal) => void;
}

/**
 * Owns the DOM-sensitive xterm lifecycle shared by every terminal surface.
 *
 * xterm's renderer and viewport may access dimensions asynchronously while
 * open/fit/ResizeObserver callbacks are running. A host must therefore:
 * - delay open until the element is connected and measurable;
 * - coalesce fit requests into one RAF;
 * - cancel every pending RAF/observer before disposing the terminal;
 * - never call fit after the instance has been disposed.
 *
 * Session-specific input/output behavior stays outside this helper.
 */
export function mountXtermHost(
  host: HTMLDivElement,
  options: XtermHostLifecycleOptions,
): XtermHostLifecycle {
  let disposed = false;
  let initRaf: number | null = null;
  let fitRaf: number | null = null;
  let bootstrapObserver: ResizeObserver | null = null;
  let resizeObserver: ResizeObserver | null = null;
  let instance: XtermHostInstance | null = null;

  const measurable = () => (
    host.isConnected
    && host.clientWidth > 0
    && host.clientHeight > 0
  );

  const scheduleFit = () => {
    if (disposed || !instance) return;
    if (fitRaf !== null) cancelAnimationFrame(fitRaf);
    fitRaf = requestAnimationFrame(() => {
      fitRaf = null;
      const current = instance;
      if (disposed || !current || !measurable()) return;
      try {
        current.fitAddon.fit();
      } catch {
        return;
      }
      options.onFit?.(current.terminal);
    });
  };

  const initialize = () => {
    initRaf = null;
    if (disposed || instance || !measurable()) return;

    const next = options.create();
    next.terminal.open(host);
    if (disposed) {
      next.onDispose?.();
      next.terminal.dispose();
      return;
    }

    instance = next;
    next.onOpened?.();

    resizeObserver = new ResizeObserver(scheduleFit);
    resizeObserver.observe(host);
    bootstrapObserver?.disconnect();
    bootstrapObserver = null;
    scheduleFit();
  };

  const scheduleInitialize = () => {
    if (disposed || instance || initRaf !== null) return;
    initRaf = requestAnimationFrame(initialize);
  };

  bootstrapObserver = new ResizeObserver(scheduleInitialize);
  bootstrapObserver.observe(host);
  scheduleInitialize();

  return {
    get terminal() {
      return instance?.terminal ?? null;
    },
    scheduleFit,
    dispose() {
      if (disposed) return;
      disposed = true;
      bootstrapObserver?.disconnect();
      resizeObserver?.disconnect();
      bootstrapObserver = null;
      resizeObserver = null;

      if (initRaf !== null) {
        cancelAnimationFrame(initRaf);
        initRaf = null;
      }
      if (fitRaf !== null) {
        cancelAnimationFrame(fitRaf);
        fitRaf = null;
      }

      const current = instance;
      instance = null;
      if (current) {
        current.onDispose?.();
        current.terminal.dispose();
      }
    },
  };
}
