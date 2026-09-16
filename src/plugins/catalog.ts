import {
  installPlugins,
  type PluginDefinition,
} from "../core/plugin-registry";
import { iperfPlugin } from "./iperf";
import { localShellPlugin } from "./local-shell";
import { modbusPlugin } from "./modbus";
import { networkPlugin } from "./network";
import { rttPlugin } from "./rtt";
import { serialPlugin } from "./serial";
import { sshPlugin } from "./ssh";
import { telnetPlugin } from "./telnet";
import { tftpPlugin } from "./tftp";
import { trdpPlugin } from "./trdp";

/**
 * TauTerm 内建前端插件目录。
 *
 * 新插件只需要导出 definition 并在这里加入一次；main/App/SessionContext 不感知具体插件。
 */
export const builtinPlugins = [
  serialPlugin,
  sshPlugin,
  telnetPlugin,
  localShellPlugin,
  tftpPlugin,
  iperfPlugin,
  networkPlugin,
  trdpPlugin,
  modbusPlugin,
  rttPlugin,
] as const satisfies readonly PluginDefinition[];

export function installBuiltinPlugins(): void {
  installPlugins(builtinPlugins);
}
