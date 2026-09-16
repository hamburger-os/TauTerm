import { invoke } from "@tauri-apps/api/core";
import { homeDir } from "@tauri-apps/api/path";
import { createElement } from "react";
import { StatusBarBadge } from "../../components/Layout/StatusBarPrimitives";
import { registerPlugin, type PluginManifest } from "../../core/plugin-registry";
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
};

function normalizeLocalShellParams(params: Record<string, unknown>): Record<string, unknown> {
  return { ...DEFAULT_LOCAL_SHELL_PARAMS, ...params };
}

registerPlugin({
  manifest: manifestJson as PluginManifest,
  connectForm: LocalShellConnectForm,
  defaultConnectionParams: () => ({ ...DEFAULT_LOCAL_SHELL_PARAMS, args: [], preset_args: [] }),
  defaultSessionOptions: () => ({ transferEnabled: false, sendBarEnabled: false }),
  normalizeConnectionParams: normalizeLocalShellParams,
  prepareConnectionParams: async params => {
    const prepared = normalizeLocalShellParams(params);
    const cwd = typeof prepared.cwd === "string" ? prepared.cwd.trim() : "";
    if (cwd) return { ...prepared, cwd };
    return {
      ...prepared,
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
  canCreateElevatedSession: params => params.shell_kind !== "wsl",
  toolbarItems: [],
  rightSidebar: { available: () => false },
  statusBarItems: [
    {
      id: "local-shell-type",
      priority: 860,
      when: ({ activeTab }) => activeTab?.state === "connected" || activeTab?.state === "transferring",
      render: () => createElement(StatusBarBadge, null, "SHELL"),
    },
  ],
});
