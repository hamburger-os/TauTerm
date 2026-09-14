from pathlib import Path
import re

ROOT = Path(__file__).resolve().parents[1]


def read(path: str) -> str:
    return (ROOT / path).read_text(encoding="utf-8")


def write(path: str, content: str) -> None:
    (ROOT / path).write_text(content, encoding="utf-8")


def exact(path: str, old: str, new: str, count: int = 1) -> None:
    text = read(path)
    actual = text.count(old)
    if actual != count:
        raise SystemExit(f"{path}: expected {count} matches, found {actual}: {old[:120]!r}")
    write(path, text.replace(old, new, count))


def sub(path: str, pattern: str, replacement: str, count: int = 1) -> None:
    text = read(path)
    changed, actual = re.subn(pattern, replacement, text, count=count, flags=re.S | re.M)
    if actual != count:
        raise SystemExit(f"{path}: expected {count} regex matches, found {actual}: {pattern[:120]!r}")
    write(path, changed)


# ---------------------------------------------------------------------------
# ConnectDialog becomes a generic host for plugin-owned Serial configuration.
# ---------------------------------------------------------------------------
p = "src/components/Layout/ConnectDialog.tsx"
sub(p, r'''\nconst BAUD_RATES = .*?const FLOW_CONTROL = \[.*?\];\n''', "\n")
exact(p, '''  // 串口配置
  const [port, setPort] = useState("");
  const [baudRate, setBaudRate] = useState("115200");
  const [dataBits, setDataBits] = useState("8");
  const [parity, setParity] = useState("none");
  const [stopBits, setStopBits] = useState("1");
  const [flowControl, setFlowControl] = useState("none");
  const [dataMode, setDataMode] = useState("text");
''', '''  // 通用/共享终端配置。Serial 专属字段由 SerialConnectForm 自己拥有。
  const [port, setPort] = useState("");
  const [dataMode, setDataMode] = useState("text");
''')
exact(p, '''  const [encoding, setEncoding] = useState(DEFAULT_ENCODING);
  const [dualFrameTimeout, setDualFrameTimeout] = useState(50);
  const [transferEnabled, setTransferEnabled] = useState(true);
''', '''  const [encoding, setEncoding] = useState(DEFAULT_ENCODING);
  const [transferEnabled, setTransferEnabled] = useState(true);
''')
exact(p, '''  const [sendBarEnabled, setSendBarEnabled] = useState(true);
  const [virtualPortEnabled, setVirtualPortEnabled] = useState(false);
  const [virtualPortCount, setVirtualPortCount] = useState(1);
''', '''  const [sendBarEnabled, setSendBarEnabled] = useState(true);
''')

exact(p, '''          const p = targetTab.params;
          if (pluginRegistry.get(targetTab.pluginId)?.connectForm) setPluginParams(p);
          if (typeof p.baud_rate === "number") setBaudRate(String(p.baud_rate));
          if (typeof p.data_bits === "number") setDataBits(String(p.data_bits));
          if (typeof p.parity === "string") setParity(p.parity);
          if (typeof p.stop_bits === "string") setStopBits(p.stop_bits);
          if (typeof p.flow_control === "string") setFlowControl(p.flow_control);
          if (typeof p.data_mode === "string") setDataMode(p.data_mode);
          if (typeof p.encoding === "string") setEncoding(p.encoding);
          if (typeof p.dual_frame_timeout_ms === "number") setDualFrameTimeout(p.dual_frame_timeout_ms);
''', '''          const p = targetTab.params;
          const plugin = pluginRegistry.get(targetTab.pluginId);
          if (plugin?.connectForm) {
            setPluginParams(plugin.normalizeConnectionParams?.(p) ?? p);
          }
          if (typeof p.data_mode === "string") setDataMode(p.data_mode);
          if (typeof p.encoding === "string") setEncoding(p.encoding);
''')
exact(p, '''        if (typeof targetTab.sendBarEnabled === "boolean") setSendBarEnabled(targetTab.sendBarEnabled);
        if (typeof targetTab.virtualPortEnabled === "boolean") setVirtualPortEnabled(targetTab.virtualPortEnabled);
        if (typeof targetTab.virtualPortCount === "number") setVirtualPortCount(targetTab.virtualPortCount);
''', '''        if (typeof targetTab.sendBarEnabled === "boolean") setSendBarEnabled(targetTab.sendBarEnabled);
''')

for line in [
    '    setBaudRate("115200");\n',
    '    setDataBits("8");\n',
    '    setParity("none");\n',
    '    setStopBits("1");\n',
    '    setFlowControl("none");\n',
    '    setDualFrameTimeout(50);\n',
    '    setVirtualPortEnabled(false);\n',
    '    setVirtualPortCount(1);\n',
]:
    exact(p, line, "")

exact(p, '''  const handleModeSelect = useCallback((modeId: string) => {
    setSelectedMode(modeId);
    setPluginParams(modeId === "local-shell" ? {
      shell_mode: "auto",
      executable: "",
      args: [],
      preset_args: [],
      preset_id: "",
      shell_label: "",
      shell_kind: "native",
      wsl_distro: "",
      cwd: "",
      data_mode: "text",
      encoding: "utf-8",
      send_bar_enabled: false,
    } : {});
''', '''  const handleModeSelect = useCallback((modeId: string) => {
    setSelectedMode(modeId);
    const plugin = pluginRegistry.get(modeId);
    setPluginParams(plugin?.defaultConnectionParams?.() ?? (modeId === "local-shell" ? {
      shell_mode: "auto",
      executable: "",
      args: [],
      preset_args: [],
      preset_id: "",
      shell_label: "",
      shell_kind: "native",
      wsl_distro: "",
      cwd: "",
      data_mode: "text",
      encoding: "utf-8",
      send_bar_enabled: false,
    } : {}));
''')

sub(p, r'''    let params: Record<string, unknown> = isSerial \? \{.*?    \} : isSsh \? \{''', '''    let params: Record<string, unknown> = isSerial
      ? (pluginRegistry.get("serial")?.normalizeConnectionParams?.(pluginParams) ?? pluginParams)
      : isSsh ? {''')

# Remove the legacy inline Serial form. Serial is rendered through PluginConnectForm below.
sub(p, r'''\n                    \{/\* ── 串口配置 ── \*/\}.*?\n                    \{/\* ── SSH 配置 ── \*/\}''', '''
                    {/* ── SSH 配置 ── */}''')

exact(p, '''                    {PluginConnectForm && (
                      <PluginConnectForm
                        params={pluginParams}
                        onChange={setPluginParams}
                        endpoints={state.endpoints.filter(endpoint => endpoint.connection_type === selectedMode)}
                      />
                    )}
''', '''                    {PluginConnectForm && (
                      <PluginConnectForm
                        params={pluginParams}
                        onChange={setPluginParams}
                        endpoints={state.endpoints.filter(endpoint => endpoint.connection_type === selectedMode)}
                        endpoint={isSerial ? port : undefined}
                        onEndpointChange={isSerial ? setPort : undefined}
                        onRefreshEndpoints={selectedPlugin?.manifest.capabilities.includes("endpoint_discovery")
                          ? () => void refreshModeEndpoints(selectedMode, true)
                          : undefined}
                        refreshingEndpoints={refreshingEndpoints}
                        disabled={connecting}
                        sessionOptions={{ transferEnabled, transferProtocol, sendBarEnabled }}
                        onSessionOptionsChange={options => {
                          setTransferEnabled(options.transferEnabled);
                          setTransferProtocol((options.transferProtocol ?? "ymodem") as "ymodem" | "xmodem" | "zmodem");
                          setSendBarEnabled(options.sendBarEnabled);
                        }}
                      />
                    )}
''')

# Drop deleted serial state names from the handleCreate dependency array.
text = read(p)
for name in ["baudRate", "dataBits", "parity", "stopBits", "flowControl", "dualFrameTimeout", "virtualPortEnabled", "virtualPortCount"]:
    text = re.sub(rf''',\s*{name}(?=,|\])''', "", text)
write(p, text)

# ---------------------------------------------------------------------------
# StatusBar delegates Serial-specific rendering to plugin statusBarItems.
# ---------------------------------------------------------------------------
p = "src/components/Layout/StatusBar.tsx"
exact(p, 'import { useCom0comStatus } from "../../hooks/useCom0comStatus";\n', "")
exact(p, 'import { formatBytes, formatUptime, formatPortParams, formatRate } from "../../utils/format";\n', 'import { formatBytes, formatUptime, formatRate } from "../../utils/format";\n')
for line in [
    '  serialParams: 880,\n',
    '  signalLines: 870,\n',
    '  vport: 300,\n',
]:
    exact(p, line, "")
sub(p, r'''\n  const \{\n    driverMissing,.*?\n  \} = useCom0comStatus\(\);\n''', "\n")
exact(p, '  const isSerial = activeTab?.pluginId === "serial";\n', "")
sub(p, r'''\n    isConnected && isSerial && params.*?\n    isConnected && isSsh''', '''
    isConnected && isSsh''')
sub(p, r'''\n    activeTab && isConnected && isSerial && activeTab\.virtualVirtualEndpoints.*?\n    loggingSessions\.size > 0''', '''
    loggingSessions.size > 0''')

# ---------------------------------------------------------------------------
# SessionContext: params are the sole Serial virtual-port configuration truth;
# plugin normalization is enforced at all persistence/connect entry points.
# ---------------------------------------------------------------------------
p = "src/context/SessionContext.tsx"
exact(p, '''  /** 是否启用虚拟串口（默认 true） */
  virtualPortEnabled?: boolean;
  /** 虚拟端口对数量（默认 1） */
  virtualPortCount?: number;
''', "")
exact(p, '''                virtualPortEnabled: (action.params?.virtual_port_enabled as boolean) ?? t.virtualPortEnabled,
                virtualPortCount: (action.params?.virtual_port_count as number) ?? t.virtualPortCount,
''', "")
exact(p, '''          virtualPortEnabled: (params.virtual_port_enabled as boolean) ?? false,
          virtualPortCount: (params.virtual_port_count as number) ?? 0,
''', "")
exact(p, '''        virtual_port_enabled?: boolean;
        virtual_port_count?: number;
''', "")
exact(p, '''            virtualPortEnabled: s.virtual_port_enabled ?? false,
            virtualPortCount: s.virtual_port_count ?? 0,
''', "")

# Normalize params before runtime connect.
exact(p, '''    const effectivePluginId = pluginId || "serial";
    const effectiveSendBarEnabled = pluginRegistry.resolveSendBarEnabled(effectivePluginId, sendBarEnabled);
''', '''    const effectivePluginId = pluginId || "serial";
    const effectiveParams = pluginRegistry.get(effectivePluginId)?.normalizeConnectionParams?.(params) ?? params;
    const effectiveSendBarEnabled = pluginRegistry.resolveSendBarEnabled(effectivePluginId, sendBarEnabled);
''')
exact(p, '''        endpoint, params, name,
        pluginId: effectivePluginId,
''', '''        endpoint, params: effectiveParams, name,
        pluginId: effectivePluginId,
''')

# Normalize before offline persistence/name derivation and store normalized params in TabInfo.
exact(p, '''      const plugin = pluginRegistry.get(pid);
      const effectiveSendBarEnabled = pluginRegistry.resolveSendBarEnabled(pid, sendBarEnabled);
      // 默认会话名只在创建时计算一次；后续配置变化只更新动态摘要，不改会话身份。
''', '''      const plugin = pluginRegistry.get(pid);
      const normalizedParams = plugin?.normalizeConnectionParams?.(params) ?? params;
      const effectiveSendBarEnabled = pluginRegistry.resolveSendBarEnabled(pid, sendBarEnabled);
      // 默认会话名只在创建时计算一次；后续配置变化只更新动态摘要，不改会话身份。
''')
exact(p, '      const presentationName = plugin?.sessionPresentation?.defaultName?.(params, endpoint)?.trim();\n', '      const presentationName = plugin?.sessionPresentation?.defaultName?.(normalizedParams, endpoint)?.trim();\n')
# Only first createOffline save payload occurrence after this point should change. Use scoped slice.
text = read(p)
marker = '  const createOfflineSession = useCallback'
start = text.index(marker)
end = text.index('  const disconnect = useCallback', start)
segment = text[start:end]
if segment.count('        endpoint, params,\n') != 1:
    raise SystemExit('SessionContext createOffline save payload anchor mismatch')
segment = segment.replace('        endpoint, params,\n', '        endpoint, params: normalizedParams,\n', 1)
segment = segment.replace('      const persistedParams = persistedSessionParams(pid, sessionId, params);\n', '      const persistedParams = persistedSessionParams(pid, sessionId, normalizedParams);\n', 1)
text = text[:start] + segment + text[end:]
write(p, text)

# Normalize reconfiguration before persistence and reducer update.
exact(p, '''    const effectiveSendBarEnabled = pluginRegistry.resolveSendBarEnabled(effectivePluginId, sendBarEnabled);
    try {
      await invoke("save_session_config", {
''', '''    const plugin = pluginRegistry.get(effectivePluginId);
    const normalizedParams = plugin?.normalizeConnectionParams?.(params) ?? params;
    const effectiveSendBarEnabled = pluginRegistry.resolveSendBarEnabled(effectivePluginId, sendBarEnabled);
    try {
      await invoke("save_session_config", {
''')
# scoped reconfigure substitutions
text = read(p)
start = text.index('  const reconfigureSession = useCallback')
end = text.index('  const openChannel = useCallback', start)
segment = text[start:end]
if segment.count('        params,\n') < 1:
    raise SystemExit('SessionContext reconfigure params anchor missing')
segment = segment.replace('        params,\n', '        params: normalizedParams,\n', 1)
segment = segment.replace('    const persistedParams = persistedSessionParams(effectivePluginId, sessionId, params);\n', '    const persistedParams = persistedSessionParams(effectivePluginId, sessionId, normalizedParams);\n', 1)
text = text[:start] + segment + text[end:]
write(p, text)

# Normalize saved library params on load. No aliases/migrations: unknown Serial fields are simply
# excluded by the current plugin schema.
exact(p, '''        const tabs: TabInfo[] = saved.map((s) => {
          const pluginId = s.plugin_id || "serial";
          return {
''', '''        const tabs: TabInfo[] = saved.map((s) => {
          const pluginId = s.plugin_id || "serial";
          const params = pluginRegistry.get(pluginId)?.normalizeConnectionParams?.(s.params) ?? s.params;
          return {
''')
exact(p, '            params: s.params,\n', '            params,\n')

print("serial frontend refactor applied")
