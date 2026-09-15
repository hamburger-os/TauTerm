import { createElement } from "react";
import { StatusBarBadge } from "../../components/Layout/StatusBarPrimitives";
import { registerPlugin, type PluginManifest } from "../../core/plugin-registry";
import manifestJson from "../../plugin-manifests/local-shell.json";
import LocalShellConnectForm from "./LocalShellConnectForm";

registerPlugin({
  manifest: manifestJson as PluginManifest,
  connectForm: LocalShellConnectForm,
  toolbarItems: [],
  statusBarItems: [
    {
      id: "local-shell-type",
      priority: 860,
      when: ({ activeTab }) => activeTab?.state === "connected" || activeTab?.state === "transferring",
      render: () => createElement(StatusBarBadge, null, "SHELL"),
    },
  ],
});
