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
