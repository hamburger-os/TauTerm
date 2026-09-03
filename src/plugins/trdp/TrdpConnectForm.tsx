import type { ConnectFormProps } from "../../core/plugin-registry";

const field: React.CSSProperties = { display: "grid", gap: 6, marginBottom: 14 };
const input: React.CSSProperties = { width: "100%" };

function str(params: Record<string, unknown>, key: string, fallback = "") {
  const value = params[key];
  return typeof value === "string" ? value : fallback;
}

function bool(params: Record<string, unknown>, key: string, fallback = false) {
  const value = params[key];
  return typeof value === "boolean" ? value : fallback;
}

export default function TrdpConnectForm({ params, onChange }: ConnectFormProps) {
  const mode = str(params, "mode", "node") as "node" | "monitor";
  const patch = (next: Record<string, unknown>) => onChange({
    mode: "node",
    link_a_ip: "0.0.0.0",
    link_b_enabled: false,
    link_b_ip: "0.0.0.0",
    pd_port: 17224,
    md_udp_port: 17225,
    md_tcp_port: 17225,
    capture_filter: "udp port 17224 or udp port 17225 or tcp port 17225",
    ...params,
    ...next,
  });

  return (
    <div>
      <div style={field}>
        <label>会话模式 / Session mode</label>
        <select className="liquid-glass-input liquid-glass-select" style={input} value={mode} onChange={e => patch({ mode: e.target.value })}>
          <option value="node">节点 / Node — PD Publisher/Subscriber + MD</option>
          <option value="monitor">监控 / Monitor — Passive capture & pcap analysis</option>
        </select>
      </div>

      {mode === "node" ? (
        <>
          <div style={field}>
            <label>链路 A 本机 IP / Link A local IP</label>
            <input className="liquid-glass-input" style={input} value={str(params, "link_a_ip", "0.0.0.0")} onChange={e => patch({ link_a_ip: e.target.value })} placeholder="10.0.0.10" />
            <small>建议填写具体接口 IPv4；0.0.0.0 仅用于实验环境。</small>
          </div>
          <label className="liquid-glass-toggle" style={{ marginBottom: 14 }}>
            <input type="checkbox" checked={bool(params, "link_b_enabled")} onChange={e => patch({ link_b_enabled: e.target.checked })} />
            <div />
            <span>启用 A/B 双链路 / Enable Link B</span>
          </label>
          {bool(params, "link_b_enabled") && (
            <div style={field}>
              <label>链路 B 本机 IP / Link B local IP</label>
              <input className="liquid-glass-input" style={input} value={str(params, "link_b_ip", "0.0.0.0")} onChange={e => patch({ link_b_ip: e.target.value })} placeholder="10.0.1.10" />
            </div>
          )}
          <div style={field}>
            <label>TRDP XML（可选）/ TRDP XML (optional)</label>
            <input className="liquid-glass-input" style={input} value={str(params, "xml_path")} onChange={e => patch({ xml_path: e.target.value })} placeholder="C:\\project\\trdp_config.xml" />
            <small>连接后导入并预览；发送对象始终默认 Stopped。</small>
          </div>
          <details>
            <summary>高级 / Advanced</summary>
            <div style={{ ...field, marginTop: 10 }}>
              <label>TCNOpen bridge 路径 / Bridge executable</label>
              <input className="liquid-glass-input" style={input} value={str(params, "bridge_path")} onChange={e => patch({ bridge_path: e.target.value })} placeholder="auto" />
              <small>未填写时从 TauTerm 可执行文件同目录和 src-tauri/binaries 自动发现。</small>
            </div>
          </details>
        </>
      ) : (
        <>
          <div style={field}>
            <label>抓包接口 / Capture interface</label>
            <input className="liquid-glass-input" style={input} value={str(params, "capture_interface")} onChange={e => patch({ capture_interface: e.target.value })} placeholder="Windows: \\Device\\NPF_{GUID}; Linux/macOS: en0/eth0" />
          </div>
          <div style={field}>
            <label>默认过滤器 / Capture filter</label>
            <input className="liquid-glass-input" style={input} value={str(params, "capture_filter", "udp port 17224 or udp port 17225 or tcp port 17225")} onChange={e => patch({ capture_filter: e.target.value })} />
            <small>默认仅 TRDP 标准端口：PD UDP/17224；MD UDP/TCP/17225。</small>
          </div>
          <p style={{ opacity: 0.78, fontSize: 12 }}>
            Windows 实时 Monitor 需要单独安装 Npcap；TauTerm 不捆绑 Npcap。离线打开 .pcap/.pcapng 不需要 Npcap。
          </p>
        </>
      )}
    </div>
  );
}
