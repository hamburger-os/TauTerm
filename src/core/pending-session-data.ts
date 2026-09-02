/**
 * Bounded startup buffer for terminal bytes that arrive before xterm is ready.
 * Data can arrive before the tab or xterm write callback is registered. Keep the
 * original chunk ordering, cap memory per session, and discard data after drain.
 */
export class PendingSessionData {
  private readonly sessions = new Map<string, { chunks: Uint8Array[]; bytes: number }>();
  private readonly maxBytesPerSession: number;
  private readonly maxSessions: number;

  constructor(maxBytesPerSession = 1024 * 1024, maxSessions = 64) {
    this.maxBytesPerSession = maxBytesPerSession;
    this.maxSessions = maxSessions;
  }

  push(sessionId: string, data: Uint8Array): void {
    if (data.length === 0 || this.maxBytesPerSession <= 0 || this.maxSessions <= 0) return;
    let pending = this.sessions.get(sessionId);
    if (!pending) {
      if (this.sessions.size >= this.maxSessions) {
        const oldest = this.sessions.keys().next().value as string | undefined;
        if (oldest !== undefined) this.sessions.delete(oldest);
      }
      pending = { chunks: [], bytes: 0 };
      this.sessions.set(sessionId, pending);
    }
    const remaining = this.maxBytesPerSession - pending.bytes;
    if (remaining <= 0) return;
    const chunk = data.length <= remaining ? data.slice() : data.slice(0, remaining);
    pending.chunks.push(chunk);
    pending.bytes += chunk.length;
  }

  drain(sessionId: string): Uint8Array[] {
    const pending = this.sessions.get(sessionId);
    if (!pending) return [];
    this.sessions.delete(sessionId);
    return pending.chunks;
  }

  delete(sessionId: string): void {
    this.sessions.delete(sessionId);
  }
}
