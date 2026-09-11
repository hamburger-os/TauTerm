import { registerPlugin, type PluginManifest } from "../../core/plugin-registry";
import manifestJson from "../../plugin-manifests/modbus.json";
import ModbusConnectForm from "./ModbusConnectForm";
import ModbusSessionView from "./ModbusSessionView";

registerPlugin({
  manifest: manifestJson as PluginManifest,
  connectForm: ModbusConnectForm,
  customView: ModbusSessionView,
});

console.log("[Plugin] Modbus plugin registered");
