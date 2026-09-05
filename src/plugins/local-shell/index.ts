import { registerPlugin } from "../../core/plugin-registry";
import LocalShellConnectForm from "./LocalShellConnectForm";

registerPlugin({
  manifest: {
    id: "local-shell",
    name: "Local Shell",
    version: "1.0.0",
    category: "terminal",
    description: "Local Shell",
    icon: "ssh-shell",
    content_type: "terminal",
    send_bar: false,
    capabilities: ["connection", "endpoint_discovery", "multi_session", "elevated_session"],
    transfer_protocols: [],
  },
  connectForm: LocalShellConnectForm,
  toolbarItems: [],
});
