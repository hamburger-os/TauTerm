import { registerPlugin, type PluginManifest } from "../../core/plugin-registry";
import manifestJson from "../../plugin-manifests/modbus.json";
import ModbusConnectForm from "./ModbusConnectForm";
import ModbusSessionView from "./ModbusSessionView";
import ModbusStatusBarItem from "./ModbusStatusBarItem";

registerPlugin({
  manifest: manifestJson as PluginManifest,
  connectForm: ModbusConnectForm,
  customView: ModbusSessionView,
  statusBarItems: [
    {
      id: "modbus-runtime",
      align: "left",
      priority: 850,
      when: ({ activeTab }) => activeTab?.pluginId === "modbus" && (activeTab.state === "connected" || activeTab.state === "transferring"),
      render: context => <ModbusStatusBarItem context={context} />,
    },
  ],
});

console.log("[Plugin] Modbus plugin registered");
