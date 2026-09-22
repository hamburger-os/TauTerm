import type { Terminal } from "@xterm/xterm";
import type { FitAddon } from "@xterm/addon-fit";

export interface ManagedXTermBundle {
  terminal: Terminal;
  fitAddon: FitAddon;
}

interface ManagedXTermHostOptions {
  host: HTMLElement;
  create: () => ManagedXTermBundle;
  onOpen?: (bundle: ManagedXTermBundle) => void | (() => void);
  onFit?: (bundle: ManagedXTermBundle) => void;
}

export interface ManagedXTermHost {
  scheduleFit(): void;
  dispose(): void;
}

/**
 * Owns the DOM-sensitive xterm lifecycle shared by all terminal-like views.
 *
 * xterm may only be opened/fitted while its host is still attached and measurable.
 * ResizeObserver, StrictMode cleanup and view switches can otherwise race renderer teardown
 * and leave Viewport.syncScrollArea reading disposed renderer dimensions.
 */
export function createManagedXTermHost({
  host,
  create,
  onOpen,
  onFit,
}: ManagedXTermHostOptions): ManagedXTermHost {
  let disposed = false;
  let opened = false;
  let initRaf: number | null = null;
  let fitRaf: number | null = null;
  let bootstrapObserver: ResizeObserver | null = null;
  let resizeObserver: ResizeObserver | null = null;
  let bundle: ManagedXTermBundle | null = null;
  let openCleanup: (() => void) | null = null;

  const measurable = () => (
    host.isConnected
    && host.clientWidth > 0
    && host.clientHeight > 0
  );

  const scheduleFit = () => {
    if (disposed || !opened || !bundle) return;
    if (fitRaf !== null) cancelAnimationFrame(fitRaf);
    fitRaf = requestAnimationFrame(() => {
      fitRaf = null;
      if (disposed || !opened || !bundle || !measurable()) return;
      try {
        bundle.fitAddon.fit();
      } catch {
        return;
      }
      onFit?.(bundle);
    });
  };

  const initialize = () => {
    initRaf = null;
    if (disposed || opened || !measurable()) return;

    const next = create();
    bundle = next;
    try {
      next.terminal.open(host);
    } catch (error) {
      next.terminal.dispose();
      bundle = null;
      throw error;
    }
    if (disposed) {
      next.terminal.dispose();
      bundle = null;
      return;
    }

    opened = true;
    openCleanup = onOpen?.(next) ?? null;
    resizeObserver = new ResizeObserver(scheduleFit);
    resizeObserver.observe(host);
    bootstrapObserver?.disconnect();
    bootstrapObserver = null;
    scheduleFit();
  };

  const scheduleInitialize = () => {
    if (disposed || opened || initRaf !== null) return;
    initRaf = requestAnimationFrame(initialize);
  };

  bootstrapObserver = new ResizeObserver(scheduleInitialize);
  bootstrapObserver.observe(host);
  scheduleInitialize();

  return {
    scheduleFit,
    dispose() {
      if (disposed) return;
      disposed = true;
      bootstrapObserver?.disconnect();
      resizeObserver?.disconnect();
      bootstrapObserver = null;
      resizeObserver = null;
      if (initRaf !== null) cancelAnimationFrame(initRaf);
      if (fitRaf !== null) cancelAnimationFrame(fitRaf);
      initRaf = null;
      fitRaf = null;
      try {
        openCleanup?.();
      } finally {
        openCleanup = null;
        bundle?.terminal.dispose();
        bundle = null;
        opened = false;
      }
    },
  };
}
