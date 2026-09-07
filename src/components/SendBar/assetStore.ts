import { invoke } from "@tauri-apps/api/core";

export const ASSET_KEYS = {
  commandSets: "assets.command_sets",
  activeCommandSet: "assets.active_command_set",
  autoReplyConfigs: "assets.auto_reply_configs",
  activeAutoReplyConfig: "assets.active_auto_reply_config",
  scripts: "assets.scripts",
  activeScriptId: "assets.active_script_id",
} as const;

export const ASSET_PERSISTENCE_ERROR_EVENT = "tauterm-asset-persistence-error";

export interface AssetPersistenceErrorDetail {
  key: string;
  error: string;
}

type AssetListener = (value: unknown | null) => void;

const cache = new Map<string, unknown | null>();
const loadPromises = new Map<string, Promise<unknown | null>>();
const writeQueues = new Map<string, Promise<void>>();
const listeners = new Map<string, Set<AssetListener>>();

let lastErrorNoticeAt = 0;

function notify(key: string, value: unknown | null) {
  listeners.get(key)?.forEach(listener => listener(value));
}

function reportPersistenceError(key: string, error: unknown) {
  const detail: AssetPersistenceErrorDetail = {
    key,
    error: String(error),
  };
  console.error(`Failed to persist engineering asset ${key}:`, error);

  // One disk/backend failure can affect several asset keys at once. Keep it visible without
  // multiplying identical toasts from every write in the same failure burst.
  const now = Date.now();
  if (now - lastErrorNoticeAt >= 2000) {
    lastErrorNoticeAt = now;
    window.dispatchEvent(
      new CustomEvent<AssetPersistenceErrorDetail>(ASSET_PERSISTENCE_ERROR_EVENT, { detail }),
    );
  }
}

function enqueueWrite(key: string, operation: () => Promise<void>) {
  const previous = writeQueues.get(key) ?? Promise.resolve();
  const task = previous
    .catch(() => {
      // A prior write already reported its own failure; later user edits must still be retriable.
    })
    .then(operation);

  writeQueues.set(key, task);
  void task.finally(() => {
    if (writeQueues.get(key) === task) {
      writeQueues.delete(key);
    }
  }).catch(() => {
    // operation() reports the failure; suppress the bookkeeping promise rejection.
  });
}

export function loadAsset<T>(key: string): Promise<T | null> {
  if (cache.has(key)) {
    return Promise.resolve(cache.get(key) as T | null);
  }

  const pending = loadPromises.get(key);
  if (pending) {
    return pending as Promise<T | null>;
  }

  const load = invoke<T | null>("get_config", { key })
    .then(value => {
      cache.set(key, value);
      return value;
    })
    .finally(() => {
      loadPromises.delete(key);
    });

  loadPromises.set(key, load as Promise<unknown | null>);
  return load;
}

export function persistAsset(key: string, value: unknown): void {
  enqueueWrite(key, async () => {
    const previous = cache.has(key) ? cache.get(key) ?? null : null;
    try {
      await invoke("set_config", { key, value });
      cache.set(key, value);
      notify(key, value);
    } catch (error) {
      notify(key, previous);
      reportPersistenceError(key, error);
      throw error;
    }
  });
}

export function clearAsset(key: string): void {
  enqueueWrite(key, async () => {
    const previous = cache.has(key) ? cache.get(key) ?? null : null;
    try {
      await invoke("delete_config", { key });
      cache.set(key, null);
      notify(key, null);
    } catch (error) {
      notify(key, previous);
      reportPersistenceError(key, error);
      throw error;
    }
  });
}

export function subscribeAsset<T>(
  key: string,
  listener: (value: T | null) => void,
): () => void {
  let bucket = listeners.get(key);
  if (!bucket) {
    bucket = new Set();
    listeners.set(key, bucket);
  }
  const wrapped: AssetListener = value => listener(value as T | null);
  bucket.add(wrapped);

  return () => {
    const current = listeners.get(key);
    current?.delete(wrapped);
    if (current?.size === 0) {
      listeners.delete(key);
    }
  };
}
