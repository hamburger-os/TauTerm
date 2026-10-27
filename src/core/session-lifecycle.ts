/** Connection health is independent of task activity. */
export type ConnectionStatus = "disconnected" | "connecting" | "connected" | "disconnecting";

/** Rust serializes its monotonic u64 runtime identity as a decimal string. */
export function compareRuntimeIds(first: string, second: string): number {
  const left = BigInt(first);
  const right = BigInt(second);
  return left < right ? -1 : left > right ? 1 : 0;
}

/** Invalidating a scope also invalidates every outstanding request; tokens never repeat. */
export class AsyncRequestScope {
  private scope: object = {};
  private request: object | null = null;

  invalidate(): void {
    this.scope = {};
    this.request = null;
  }

  begin(): () => boolean {
    const scope = this.scope;
    const request = {};
    this.request = request;
    return () => this.scope === scope && this.request === request;
  }
}

/** An observer cursor rejects delayed/duplicate lifecycle events before invalidating its work. */
export class RuntimeEventCursor {
  private runtimeId: string | null = null;
  private connected = false;

  matches(runtimeId: string): boolean { return this.connected && this.runtimeId === runtimeId; }

  accept(runtimeId: string, connected: boolean): boolean {
    if (this.runtimeId) {
      const order = compareRuntimeIds(runtimeId, this.runtimeId);
      if (order < 0 || (order === 0 && (this.connected === connected || connected))) return false;
    }
    this.runtimeId = runtimeId;
    this.connected = connected;
    return true;
  }
}
