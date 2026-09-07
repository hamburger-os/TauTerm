import { invoke } from "@tauri-apps/api/core";

export const ASSET_KEYS = {
  commandSets: "assets.command_sets",
  activeCommandSet: "assets.active_command_set",
  autoReplyConfigs: "assets.auto_reply_configs",
  activeAutoReplyConfig: "assets.active_auto_reply_config",
  scripts: "assets.scripts",
  activeScriptId: "assets.active_script_id",
} as const;

export function loadAsset<T>(key: string): Promise<T | null> {
  return invoke<T | null>("get_config", { key });
}

export function persistAsset(key: string, value: unknown): void {
  void invoke("set_config", { key, value }).catch(() => {});
}

export function clearAsset(key: string): void {
  void invoke("delete_config", { key }).catch(() => {});
}
