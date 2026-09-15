/**
 * Dependency-free frontend plugin contracts.
 *
 * This module deliberately has no React, i18n or SessionContext dependencies. Shared
 * presentation/configuration contracts live here so the plugin registry remains the only
 * runtime registry and common UI code never needs a mirrored protocol registry.
 */

/** Creation-time identity and dynamic configuration summary owned by the plugin. */
export interface SessionPresentation {
  /** Stable default name generated once when a saved Session is created. */
  defaultName?: (params: Record<string, unknown>, endpoint: string) => string;
  /** Dynamic second-line summary derived from the current Session configuration. */
  subtitle?: (params: Record<string, unknown>, endpoint: string) => string;
}

/** Minimal dependency-free input for resolving the common Session subtitle fallback. */
export interface SessionPresentationSnapshot {
  endpoint: string;
  params?: Record<string, unknown>;
  parentId?: string | null;
}

/**
 * Resolve a Session subtitle without importing the runtime registry or any UI dependency.
 * Runtime UI code supplies the plugin-owned descriptor from PluginRegistry; architecture/product
 * tests can exercise the same resolution contract directly under Node.
 */
export function resolveSessionSubtitle(
  presentation: SessionPresentation | undefined,
  session: SessionPresentationSnapshot,
): string {
  if (session.parentId) return session.endpoint;
  const subtitle = presentation
    ?.subtitle?.(session.params ?? {}, session.endpoint)
    ?.trim();
  return subtitle || session.endpoint;
}

/**
 * Result of a plugin-owned reconnect preflight. The common Session layer only consumes the
 * decision and message; protocol-specific safety checks stay inside the plugin.
 */
export type SessionReconnectGuardResult =
  | { ok: true }
  | { ok: false; message: string };

export interface SessionReconnectContext {
  sessionId: string;
  endpoint: string;
  params: Record<string, unknown>;
}
