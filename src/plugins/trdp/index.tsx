import { registerPlugin } from "../../core/plugin-registry";
import TrdpConnectForm from "./TrdpConnectForm";
import TrdpSessionView from "./TrdpSessionView";

registerPlugin({
  manifest: {
    id: "trdp",
    name: "TRDP",
    version: "1.0.0",
    category: "network_tool",
    description: "TRDP · Train Real Time Data Protocol",
    icon: "globe",
    content_type: "custom",
    send_bar: false,
    capabilities: ["connection", "network_outbound", "network_listen"],
    transfer_protocols: [],
  },
  connectForm: TrdpConnectForm,
  customView: TrdpSessionView,
});

console.log("[Plugin] TRDP plugin registered");
