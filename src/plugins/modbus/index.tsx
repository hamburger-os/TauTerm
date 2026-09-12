import { registerPlugin, type PluginManifest } from "../../core/plugin-registry";
import manifestJson from "../../plugin-manifests/modbus.json";
import ModbusConnectForm from "./ModbusConnectForm";
import ModbusSessionView from "./ModbusSessionView";
import ModbusStatusBarItem from "./ModbusStatusBarItem";
import { modbusLocales } from "./locales";
import { normalizeModbusSessionParams } from "./model";

registerPlugin({
  manifest: manifestJson as PluginManifest,
  connectForm: ModbusConnectForm,
  isConnectionConfigValid: params => {
    const config = normalizeModbusSessionParams(params);
    if (config.mode === "tcp") {
      return config.host.trim().length > 0 && config.port >= 1 && config.port <= 65535;
    }
    return config.serial_port.trim().length > 0;
  },
  customView: ModbusSessionView,
  locales: modbusLocales,
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
