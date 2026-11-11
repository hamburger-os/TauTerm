import { invoke } from "@tauri-apps/api/core";
import { homeDir } from "@tauri-apps/api/path";
import { createElement } from "react";
import { StatusBarBadge } from "../../components/Layout/StatusBarPrimitives";
import { definePlugin, type PluginManifest } from "../../core/plugin-registry";
import i18n from "../../i18n";
import manifestJson from "../../plugin-manifests/local-shell.json";
import LocalShellConnectForm from "./LocalShellConnectForm";

const DEFAULT_LOCAL_SHELL_PARAMS: Record<string, unknown> = {
  shell_mode: "auto",
  executable: "",
  args: [],
  preset_args: [],
  preset_id: "",
  shell_label: "",
  shell_kind: "native",
  wsl_distro: "",
  cwd: "",
  data_mode: "text",
  encoding: "utf-8",
  send_bar_enabled: false,
  env: [],
};

function normalizeLocalShellParams(params: Record<string, unknown>): Record<string, unknown> {
  return { ...DEFAULT_LOCAL_SHELL_PARAMS, ...params };
}

/**
 * The environment block belongs to native shells only. Empty rows are editing
 * artifacts and are dropped at connect time; WSL rows are dropped because the
 * subsystem owns its environment and the backend rejects the field there.
 */
function normalizeEnvironmentRows(
  value: unknown,
  shellKind: unknown,
): Array<{ key: string; value: string }> {
  if (shellKind === "wsl" || !Array.isArray(value)) return [];
  return value
    .filter((entry): entry is Record<string, unknown> => typeof entry === "object" && entry !== null)
    .map(entry => ({
      key: typeof entry.key === "string" ? entry.key.trim() : "",
      value: typeof entry.value === "string" ? entry.value : "",
    }))
    .filter(entry => entry.key.length > 0);
}

function formatLocalShellError(error: unknown): string {
  const raw = String(error);
  if (raw.includes("User cancelled the UAC elevation prompt")) {
    return i18n.t("localShell.elevationCancelled");
  }
  if (raw.includes("Environment variable names may only contain")) {
    return i18n.t("localShell.envKeyInvalid");
  }
  if (raw.includes("Environment variable values cannot contain NUL")) {
    return i18n.t("localShell.envValueInvalid");
  }
  if (raw.includes("Too many environment variables")) {
    return i18n.t("localShell.envTooMany");
  }
  if (raw.includes("Environment variables are not supported for WSL shells")) {
    return i18n.t("localShell.envWslUnsupported");
  }
  return raw;
}

export const localShellPlugin = definePlugin({
  manifest: manifestJson as PluginManifest,
  connectForm: LocalShellConnectForm,
  defaultConnectionParams: () => ({ ...DEFAULT_LOCAL_SHELL_PARAMS, args: [], preset_args: [] }),
  defaultSessionOptions: () => ({ transferEnabled: false, sendBarEnabled: false }),
  normalizeConnectionParams: normalizeLocalShellParams,
  prepareConnectionParams: async params => {
    const prepared = normalizeLocalShellParams(params);
    const cwd = typeof prepared.cwd === "string" ? prepared.cwd.trim() : "";
    const normalized = {
      ...prepared,
      env: normalizeEnvironmentRows(prepared.env, prepared.shell_kind),
    };
    if (cwd) return { ...normalized, cwd };
    return {
      ...normalized,
      cwd: prepared.shell_kind === "wsl" ? "~" : await homeDir(),
    };
  },
  isConnectionConfigValid: params => {
    const normalized = normalizeLocalShellParams(params);
    if (normalized.shell_mode !== "custom") return true;
    return typeof normalized.executable === "string" && normalized.executable.trim().length > 0;
  },
  resolveEndpoint: params => String(params.cwd ?? "").trim(),
  sessionPresentation: {
    defaultName: (_params, endpoint) => `Shell @ ${endpoint}`,
    subtitle: (_params, endpoint) => endpoint,
  },
  resolveDefaultSessionName: params => invoke<string>("resolve_local_shell_session_name", { params }),
  formatSessionError: (error, operation) => {
    const detail = formatLocalShellError(error);
    const key = operation === "open_channel" ? "localShell.openFailed" : "localShell.connectFailed";
    return `${i18n.t(key)}: ${detail}`;
  },
  canCreateElevatedSession: params => params.shell_kind !== "wsl",
  toolbarItems: [],
  rightSidebar: { available: () => false },
  statusBarItems: [
    {
      id: "local-shell-type",
      priority: 860,
      when: ({ activeTab }) => activeTab?.state === "connected",
      render: () => createElement(StatusBarBadge, null, "SHELL"),
    },
  ],
});
