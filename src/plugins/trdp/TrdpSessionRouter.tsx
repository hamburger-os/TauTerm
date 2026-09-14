import { useSession } from "../../context/SessionContext";
import TrdpMonitorView from "./TrdpMonitorView";
import TrdpSessionView from "./TrdpSessionView";

export default function TrdpSessionRouter({ sessionId }: { sessionId: string }) {
  const { state } = useSession();
  const tab = state.tabs.find(item => item.id === sessionId);
  const mode = tab?.params?.mode === "monitor" ? "monitor" : "node";

  return mode === "monitor"
    ? <TrdpMonitorView sessionId={sessionId} />
    : <TrdpSessionView sessionId={sessionId} />;
}
