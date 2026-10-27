import type { TabInfo } from "../context/SessionContext";
import type { ProfileResolver, SessionProfile } from "./types";
import { statusIconName, statusValue } from "./status";

export const localShellProfile: ProfileResolver = (tab: TabInfo): SessionProfile => {
  const params = tab.params ?? {};
  const executable = typeof params.executable === "string" && params.executable
    ? params.executable
    : "localShell.auto";
  const shellKind = params.shell_kind === "wsl" ? "wsl" : "native";
  const shellLabel = typeof params.shell_label === "string" && params.shell_label
    ? params.shell_label
    : executable;
  const cwd = typeof params.cwd === "string" && params.cwd
    ? params.cwd
    : (shellKind === "wsl" ? "localShell.wslHomeDirectory" : "localShell.homeDirectory");
  const presetArgs = Array.isArray(params.preset_args)
    ? params.preset_args.filter((value): value is string => typeof value === "string")
    : [];
  const args = Array.isArray(params.args)
    ? params.args.filter((value): value is string => typeof value === "string")
    : [];

  return {
    identity: [
      { label: "session.renameSession", value: tab.name, icon: "tag" },
      { label: "connectionType.label", value: "connectionType.localShell", icon: "ssh-shell" },
      { label: "localShell.shell", value: shellLabel, icon: "endpoint" },
      { label: "session.status", value: statusValue(tab.state), icon: statusIconName(tab.state) },
    ],
    parameters: [
      { label: "localShell.executable", value: executable, monospace: true },
      { label: "localShell.workingDirectory", value: cwd, monospace: true },
      { label: "localShell.arguments", value: [...presetArgs, ...args].join(" ") || "—", monospace: true },
    ],
  };
};
