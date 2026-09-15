/** Telnet frontend plugin registration. */
import { createElement } from "react";
import { StatusBarBadge } from "../../components/Layout/StatusBarPrimitives";
import { registerPlugin, type PluginManifest } from "../../core/plugin-registry";
import manifestJson from "../../plugin-manifests/telnet.json";
import { telnetEndpointLabel, telnetSessionTitle } from "./presentation";
import { telnetRuntimeStore, type TelnetRuntimeSnapshot } from "./runtime-store";

registerPlugin({
  manifest: manifestJson as PluginManifest,
  sessionPresentation: {
    defaultName: () => telnetSessionTitle(),
    subtitle: (params, endpoint) => telnetEndpointLabel(params, endpoint),
  },
  runtimeStore: telnetRuntimeStore,
  terminalLocalEcho: runtimeSnapshot => (runtimeSnapshot as TelnetRuntimeSnapshot).localEcho === true,
  toolbarItems: [],
  statusBarItems: [
    {
      id: "telnet-type",
      priority: 860,
      when: ({ activeTab }) => activeTab?.state === "connected" || activeTab?.state === "transferring",
      render: () => createElement(StatusBarBadge, null, "TELNET"),
    },
  ],
  locales: {
    "zh-CN": {
      "host": "主机地址",
      "hostPlaceholder": "192.168.1.1",
      "port": "端口",
      "enableSendBar": "启用发送栏",
    },
    "en-US": {
      "host": "Host",
      "hostPlaceholder": "192.168.1.1",
      "port": "Port",
      "enableSendBar": "Enable Send Bar",
    },
  },
});

console.log("[Plugin] Telnet plugin registered");
