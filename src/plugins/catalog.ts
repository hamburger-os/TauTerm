import { installPlugins, type PluginDefinition } from "../core/plugin-registry";
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

const builtinPlugins: readonly PluginDefinition[] = [
  serialPlugin,
  sshPlugin,
  telnetPlugin,
  localShellPlugin,
  tftpPlugin,
  iperfPlugin,
  networkPlugin,
  modbusPlugin,
  trdpPlugin,
  rttPlugin,
];

export function installBuiltinPlugins(): void {
  installPlugins(builtinPlugins);
}
