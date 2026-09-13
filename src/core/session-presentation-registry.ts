/**
 * Dependency-free read model for Session card presentation.
 *
 * Workspace contract tests import the presentation helpers directly under Node, so this registry
 * intentionally has no React/i18n dependencies. The full PluginRegistry owns registration and
 * mirrors only the presentation descriptor here.
 */
export interface SessionPresentation {
  /** Creation-time default. It is not recomputed after the Session has been saved. */
  defaultName?: (params: Record<string, unknown>, endpoint: string) => string;
  /** Dynamic second-line summary derived from the current Session configuration. */
  subtitle?: (params: Record<string, unknown>, endpoint: string) => string;
}

const presentations = new Map<string, SessionPresentation>();

export function setSessionPresentation(
  pluginId: string,
  presentation?: SessionPresentation,
): void {
  if (presentation) presentations.set(pluginId, presentation);
  else presentations.delete(pluginId);
}

export function getSessionPresentation(pluginId: string): SessionPresentation | undefined {
  return presentations.get(pluginId);
}
