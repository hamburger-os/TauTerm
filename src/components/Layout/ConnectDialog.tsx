import { useState, useCallback, useEffect, useRef } from "react";
import { useTranslation } from "react-i18next";
import { motion, AnimatePresence } from "framer-motion";
import { open } from "@tauri-apps/plugin-dialog";
import { homeDir } from "@tauri-apps/api/path";
import { useSession } from "../../context/SessionContext";
import { pluginRegistry } from "../../core/plugin-registry";
import { CHARSETS, DEFAULT_ENCODING } from "../../utils/charsets";
import Icon from "../common/Icon";
import styles from "./ConnectDialog.module.css";

const BAUD_RATES = ["110","300","600","1200","2400","4800","9600","14400","19200","38400","57600","115200","230400","460800","921600"];
const DATA_BITS = ["5","6","7","8"];
const PARITY = [
  { v: "none", l: "None" },
  { v: "even", l: "Even" },
  { v: "odd", l: "Odd" },
];
const STOP_BITS = ["1","2"];
const FLOW_CONTROL = [
  { v: "none", l: "None" },
  { v: "rts_cts", l: "RTS/CTS" },
  { v: "xon_xoff", l: "XON/XOFF" },
];

interface ConnectDialogProps {
  isOpen: boolean;
  onClose: () => void;
  editSessionId?: string | null;
}

/**
 * 统一新建会话对话框
 *
 * 两步流程：
 *   1. 从 PluginRegistry 动态获取可用协议，选择连接模式
 *   2. 渲染选中插件的配置表单
 *
 * 所有已注册插件均可选——不再有 "Coming Soon" 占位。
 */
export default function ConnectDialog({ isOpen, onClose, editSessionId }: ConnectDialogProps) {
  const { t } = useTranslation();
  const { state, refreshEndpoints, switchTab, createOfflineSession, reconfigureSession } = useSession();

  const [step, setStep] = useState<"mode" | "config">("mode");
  const [selectedMode, setSelectedMode] = useState("serial");
  const [pluginParams, setPluginParams] = useState<Record<string, unknown>>({});

  // 串口配置
  const [port, setPort] = useState("");
  const [baudRate, setBaudRate] = useState("115200");
  const [dataBits, setDataBits] = useState("8");
  const [parity, setParity] = useState("none");
  const [stopBits, setStopBits] = useState("1");
  const [flowControl, setFlowControl] = useState("none");
  const [dataMode, setDataMode] = useState("text");
  /** 数据字符编码（仅终端类协议：serial/ssh/telnet；连接后不可变，改需重连） */
  const [encoding, setEncoding] = useState(DEFAULT_ENCODING);
  const [dualFrameTimeout, setDualFrameTimeout] = useState(50);
  const [transferEnabled, setTransferEnabled] = useState(true);
  const [transferProtocol, setTransferProtocol] = useState<"ymodem" | "xmodem" | "zmodem">("ymodem");
  const [sendBarEnabled, setSendBarEnabled] = useState(true);
  const [virtualPortEnabled, setVirtualPortEnabled] = useState(false);
  const [virtualPortCount, setVirtualPortCount] = useState(1);
  const [sessionName, setSessionName] = useState("");
  const [connecting, setConnecting] = useState(false);
  const [error, setError] = useState<string | null>(null);

  // SSH 配置
  const [sshHost, setSshHost] = useState("");
  const [sshPort, setSshPort] = useState(22);
  const [sshUsername, setSshUsername] = useState("");
  const [sshAuthMethod, setSshAuthMethod] = useState<"password" | "key">("password");
  const [sshPassword, setSshPassword] = useState("");
  const [sshPrivateKey, setSshPrivateKey] = useState("");
  const [sshPassphrase, setSshPassphrase] = useState("");
  const [sshSendBarEnabled, setSshSendBarEnabled] = useState(false);
  const [sshTransferEnabled, setSshTransferEnabled] = useState(false);
  const [fileServiceEnabled, setFileServiceEnabled] = useState(true);
  const [fileServiceProtocol, setFileServiceProtocol] = useState("sftp");
  const [journaldEnabled, setJournaldEnabled] = useState(false);

  const serialEndpoints = state.endpoints.filter(e => e.connection_type === "serial");
  // TFTP 配置
  const [tftpListenIp, setTftpListenIp] = useState("0.0.0.0");
  const [tftpListenPort, setTftpListenPort] = useState(69);
  const [tftpFileRoot, setTftpFileRoot] = useState("");
  const [tftpWriteEnabled, setTftpWriteEnabled] = useState(true);
  const [tftpOverwrite, setTftpOverwrite] = useState(true);
  const [tftpSinglePort, setTftpSinglePort] = useState(false);
  // Telnet 配置
  const [telnetHost, setTelnetHost] = useState("");
  const [telnetPort, setTelnetPort] = useState(23);
  const [telnetSendBarEnabled, setTelnetSendBarEnabled] = useState(true);
  // iperf 配置（服务端生命周期跟随会话：连接即按此监听参数自动启动）
  const [iperfVersion, setIperfVersion] = useState<"iperf2" | "iperf3">("iperf2");
  const [iperfListenIp, setIperfListenIp] = useState("0.0.0.0");
  const [iperfListenPort, setIperfListenPort] = useState(5001);
  // 网络调试配置（TCP/UDP 调试助手）
  const [netTransport, setNetTransport] = useState<"tcp" | "udp">("tcp");
  const [netRole, setNetRole] = useState<"client" | "server">("client");
  const [netRemoteHost, setNetRemoteHost] = useState("");
  const [netRemotePort, setNetRemotePort] = useState(8080);
  const [netLocalHost, setNetLocalHost] = useState("0.0.0.0");
  const [netLocalPort, setNetLocalPort] = useState(8080);
  const [netMaxClients, setNetMaxClients] = useState(16);
  const [netConnectTimeoutMs, setNetConnectTimeoutMs] = useState(5000);
  const [netNodelay, setNetNodelay] = useState(true);
  const [netBroadcast, setNetBroadcast] = useState(false);
  const [netMulticastGroup, setNetMulticastGroup] = useState("");
  const [netTtl, setNetTtl] = useState(64);
  const [netMulticastInterface, setNetMulticastInterface] = useState("0.0.0.0");
  const [netSelfReceive, setNetSelfReceive] = useState(true);

  const isSerial = selectedMode === "serial";
  const isSsh = selectedMode === "ssh";
  const isTftp = selectedMode === "tftp";
  const isTelnet = selectedMode === "telnet";
  const isIperf = selectedMode === "iperf";
  const isNetwork = selectedMode === "network";
  const isLocalShell = selectedMode === "local-shell";
  const selectedPlugin = pluginRegistry.get(selectedMode);
  const PluginConnectForm = selectedPlugin?.connectForm;

  // 保持最新的 tabs 引用，供 useEffect 在 editSessionId 变化时读取最新数据，
  // 避免将 state.tabs 放入依赖数组导致 session-stats 事件每秒重置表单
  const tabsRef = useRef(state.tabs);
  tabsRef.current = state.tabs;

  // 从 PluginRegistry 获取可用协议（替换硬编码列表）
  const availableModes = pluginRegistry.getByCapability("connection").map(p => ({
    id: p.manifest.id,
    icon: p.manifest.icon,
    description: p.manifest.id === "local-shell"
      ? t("connectionType.localShell")
      : (p.manifest.description || p.manifest.name),
  }));

  // 每次打开对话框时重置
  useEffect(() => {
    if (!isOpen) return;
    refreshEndpoints();
    setError(null);
    setConnecting(false);

    if (editSessionId) {
      // 使用 tabsRef 读取最新 tabs，避免将 state.tabs 放入 deps（导致 session-stats 每秒重置表单）
      const targetTab = tabsRef.current.find(t => t.id === editSessionId);
      if (targetTab) {
        setSelectedMode(targetTab.connection_type);
        setStep("config");
        if (targetTab.endpoint) setPort(targetTab.endpoint);
        if (targetTab.params) {
          const p = targetTab.params;
          if (pluginRegistry.get(targetTab.pluginId)?.connectForm) setPluginParams(p);
          if (typeof p.baud_rate === "number") setBaudRate(String(p.baud_rate));
          if (typeof p.data_bits === "number") setDataBits(String(p.data_bits));
          if (typeof p.parity === "string") setParity(p.parity);
          if (typeof p.stop_bits === "string") setStopBits(p.stop_bits);
          if (typeof p.flow_control === "string") setFlowControl(p.flow_control);
          if (typeof p.data_mode === "string") setDataMode(p.data_mode);
          if (typeof p.encoding === "string") setEncoding(p.encoding);
          if (typeof p.dual_frame_timeout_ms === "number") setDualFrameTimeout(p.dual_frame_timeout_ms);
          // SSH 字段回填
          if (targetTab.connection_type === "ssh" || targetTab.pluginId === "ssh") {
            if (typeof p.host === "string") setSshHost(p.host);
            if (typeof p.port === "number") setSshPort(p.port);
            if (typeof p.username === "string") setSshUsername(p.username);
            if (typeof p.auth_method === "string") setSshAuthMethod(p.auth_method as "password" | "key");
            if (typeof p.password === "string") setSshPassword(p.password);
            if (typeof p.private_key === "string") setSshPrivateKey(p.private_key);
            if (typeof p.passphrase === "string") setSshPassphrase(p.passphrase);
            if (typeof p.file_service_enabled === "boolean") setFileServiceEnabled(p.file_service_enabled);
            if (typeof p.send_bar_enabled === "boolean") setSshSendBarEnabled(p.send_bar_enabled);
            if (typeof p.transfer_enabled === "boolean") setSshTransferEnabled(p.transfer_enabled);
            if (typeof p.file_service_protocol === "string") setFileServiceProtocol(p.file_service_protocol);
            if (typeof p.journald_enabled === "boolean") setJournaldEnabled(p.journald_enabled);
          }
          // TFTP 字段回填
          if (targetTab.connection_type === "tftp" || targetTab.pluginId === "tftp") {
            if (typeof p.listen_ip === "string") setTftpListenIp(p.listen_ip);
            if (typeof p.listen_port === "number") setTftpListenPort(p.listen_port);
            if (typeof p.file_root === "string") setTftpFileRoot(p.file_root);
            if (typeof p.write_enabled === "boolean") setTftpWriteEnabled(p.write_enabled);
            if (typeof p.overwrite === "boolean") setTftpOverwrite(p.overwrite);
            if (typeof p.single_port === "boolean") setTftpSinglePort(p.single_port);
          }
          // Telnet 字段回填
          if (targetTab.connection_type === "telnet" || targetTab.pluginId === "telnet") {
            if (typeof p.host === "string") setTelnetHost(p.host);
            if (typeof p.port === "number") setTelnetPort(p.port);
            if (typeof p.send_bar_enabled === "boolean") setTelnetSendBarEnabled(p.send_bar_enabled);
          }
          // iperf 字段回填
          if (targetTab.connection_type === "iperf" || targetTab.pluginId === "iperf") {
            if (p.version === "iperf2" || p.version === "iperf3") setIperfVersion(p.version);
            if (typeof p.listen_ip === "string") setIperfListenIp(p.listen_ip);
            if (typeof p.listen_port === "number") setIperfListenPort(p.listen_port);
          }
          // 网络调试字段回填
          if (targetTab.connection_type === "network" || targetTab.pluginId === "network") {
            if (p.transport === "tcp" || p.transport === "udp") setNetTransport(p.transport);
            if (p.role === "client" || p.role === "server") setNetRole(p.role);
            if (typeof p.remote_host === "string") setNetRemoteHost(p.remote_host);
            if (typeof p.remote_port === "number") setNetRemotePort(p.remote_port);
            if (typeof p.local_host === "string") setNetLocalHost(p.local_host);
            if (typeof p.local_port === "number") setNetLocalPort(p.local_port);
            if (typeof p.max_clients === "number") setNetMaxClients(p.max_clients);
            if (typeof p.connect_timeout_ms === "number") setNetConnectTimeoutMs(p.connect_timeout_ms);
            if (typeof p.nodelay === "boolean") setNetNodelay(p.nodelay);
            if (typeof p.broadcast === "boolean") setNetBroadcast(p.broadcast);
            if (typeof p.multicast_group === "string") setNetMulticastGroup(p.multicast_group);
            if (typeof p.ttl === "number") setNetTtl(p.ttl);
            if (typeof p.multicast_interface === "string") setNetMulticastInterface(p.multicast_interface);
            if (typeof p.self_receive === "boolean") setNetSelfReceive(p.self_receive);
          }
        }
        if (targetTab.name) setSessionName(targetTab.name);
        if (typeof targetTab.transferEnabled === "boolean") setTransferEnabled(targetTab.transferEnabled);
        if (typeof targetTab.transferProtocol === "string") setTransferProtocol(targetTab.transferProtocol as "ymodem" | "xmodem" | "zmodem");
        if (typeof targetTab.sendBarEnabled === "boolean") setSendBarEnabled(targetTab.sendBarEnabled);
        if (typeof targetTab.virtualPortEnabled === "boolean") setVirtualPortEnabled(targetTab.virtualPortEnabled);
        if (typeof targetTab.virtualPortCount === "number") setVirtualPortCount(targetTab.virtualPortCount);
        return;
      }
    }

    setStep("mode");
    setSelectedMode("serial");
    setPluginParams({});
    setPort("");
    setBaudRate("115200");
    setDataBits("8");
    setParity("none");
    setStopBits("1");
    setFlowControl("none");
    setDataMode("text");
    setEncoding(DEFAULT_ENCODING);
    setDualFrameTimeout(50);
    setTransferEnabled(true);
    setTransferProtocol("ymodem");
    setSendBarEnabled(true);
    setVirtualPortEnabled(false);
    setVirtualPortCount(1);
    // 重置 SSH 字段
    setSshHost("");
    setSshPort(22);
    setSshUsername("");
    setSshAuthMethod("password");
    setSshPassword("");
    setSshPrivateKey("");
    setSshPassphrase("");
    setSshSendBarEnabled(false);
    setSshTransferEnabled(false);
    setFileServiceEnabled(true);
    setFileServiceProtocol("sftp");
    setJournaldEnabled(false);
    // 重置 TFTP 字段
    setTftpListenIp("0.0.0.0");
    setTftpListenPort(69);
    setTftpFileRoot("");
    setTftpWriteEnabled(true);
    setTftpOverwrite(true);
    setTftpSinglePort(false);
    // 重置 Telnet 字段
    setTelnetHost("");
    setTelnetPort(23);
    setTelnetSendBarEnabled(true);
    // 重置 iperf 字段
    setIperfVersion("iperf2");
    setIperfListenIp("0.0.0.0");
    setIperfListenPort(5001);
    // 重置网络调试字段
    setNetTransport("tcp");
    setNetRole("client");
    setNetRemoteHost("");
    setNetRemotePort(8080);
    setNetLocalHost("0.0.0.0");
    setNetLocalPort(8080);
    setNetMaxClients(16);
    setNetConnectTimeoutMs(5000);
    setNetNodelay(true);
    setNetBroadcast(false);
    setNetMulticastGroup("");
    setNetTtl(64);
    setNetMulticastInterface("0.0.0.0");
    setNetSelfReceive(true);
    setSessionName("");
  }, [isOpen, editSessionId, refreshEndpoints]);

  useEffect(() => {
    if (!isOpen || step !== "config" || editSessionId) return;
    if (serialEndpoints.length > 0 && !port) {
      setPort(serialEndpoints[0].name);
    }
  }, [isOpen, step, editSessionId, serialEndpoints, port]);

  const handleModeSelect = useCallback((modeId: string) => {
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
    setStep("config");
    setError(null);
  }, []);

  const handleBack = useCallback(() => {
    setStep("mode");
    setError(null);
  }, []);


  const handleCreate = useCallback(async () => {
    if (!port && isSerial) return;
    if (!sshHost && isSsh) return;
    if (!tftpFileRoot && isTftp) return;
    if (!telnetHost && isTelnet) return;
    if (isLocalShell && pluginParams.shell_mode === "custom" && !String(pluginParams.executable ?? "").trim()) {
      setError(t("localShell.executableRequired"));
      return;
    }
    // 网络调试：Client（TCP/UDP）必须有远端主机；Server 由本地绑定端口即可
    if (isNetwork && netRole === "client" && !netRemoteHost) return;
    setError(null);
    setConnecting(true);

    // 网络调试启用全局发送栏（发送目标由发送栏内 TargetBar 选择）
    const effectiveSendBarEnabled = isLocalShell ? false : (isSsh ? sshSendBarEnabled : (isTftp || isIperf ? false : (isTelnet ? telnetSendBarEnabled : sendBarEnabled)));
    const effectiveTransferEnabled = isSsh ? sshTransferEnabled : (isLocalShell || isTftp || isTelnet || isIperf || isNetwork ? false : transferEnabled);

    let tftpExposureConfirmed = false;
    if (isTftp && tftpWriteEnabled && tftpOverwrite) {
      const bind = tftpListenIp.trim().toLowerCase();
      const loopback = bind === "127.0.0.1" || bind === "::1" || bind === "localhost";
      if (!loopback) {
        tftpExposureConfirmed = window.confirm(t("tftp.exposureWarning", { defaultValue: "This TFTP server will accept remote writes and allow overwriting files from a non-loopback interface. Continue only on a trusted network." }));
        if (!tftpExposureConfirmed) { setConnecting(false); return; }
      }
    }

    let params: Record<string, unknown> = isSerial ? {
      baud_rate: parseInt(baudRate),
      data_bits: parseInt(dataBits),
      parity,
      stop_bits: stopBits,
      flow_control: flowControl,
      data_mode: dataMode,
      dual_frame_timeout_ms: dualFrameTimeout,
      encoding,
      transfer_enabled: transferEnabled,
      transfer_protocol: transferProtocol,
      send_bar_enabled: sendBarEnabled,
      virtual_port_enabled: virtualPortEnabled,
      virtual_port_count: virtualPortCount,
    } : isSsh ? {
      host: sshHost,
      port: sshPort,
      username: sshUsername,
      auth_method: sshAuthMethod,
      password: sshAuthMethod === "password" ? sshPassword : undefined,
      private_key: sshAuthMethod === "key" ? sshPrivateKey : undefined,
      passphrase: sshAuthMethod === "key" && sshPassphrase ? sshPassphrase : undefined,
      data_mode: dataMode,
      encoding,
      send_bar_enabled: sshSendBarEnabled,
      transfer_enabled: sshTransferEnabled,
      file_service_enabled: fileServiceEnabled,
      file_service_protocol: "sftp",
      journald_enabled: journaldEnabled,
    } : isTftp ? {
      listen_ip: tftpListenIp,
      listen_port: tftpListenPort,
      file_root: tftpFileRoot,
      write_enabled: tftpWriteEnabled,
      overwrite: tftpOverwrite,
      single_port: tftpSinglePort,
      exposure_confirmed: tftpExposureConfirmed,
    } : isTelnet ? {
      host: telnetHost,
      port: telnetPort,
      encoding,
      send_bar_enabled: telnetSendBarEnabled,
    } : isIperf ? {
      version: iperfVersion,
      listen_ip: iperfListenIp,
      listen_port: iperfListenPort,
    } : isNetwork ? {
      transport: netTransport,
      role: netRole,
      remote_host: netRemoteHost,
      remote_port: netRemotePort,
      local_host: netLocalHost,
      // 只有 server 角色需要绑定固定的本地端口；client（UDP）应绑定临时端口(0)，
      // 否则 UDP client 会继承默认的 8080，与同机 UDP server 的 0.0.0.0:8080 冲突（os error 10048）
      local_port: netRole === "server" ? netLocalPort : 0,
      max_clients: netMaxClients,
      connect_timeout_ms: netConnectTimeoutMs,
      nodelay: netNodelay,
      broadcast: netBroadcast,
      multicast_group: netMulticastGroup || undefined,
      ttl: netTtl,
      multicast_interface: netMulticastInterface,
      self_receive: netSelfReceive,
      // data_mode 仅 TCP 流视图使用；UDP 恒为报文网格双栏，不写 data_mode
      ...(netTransport === "tcp" ? { data_mode: dataMode } : {}),
      encoding,
    } : PluginConnectForm ? pluginParams : {};

    if (isLocalShell) {
      try {
        params = { ...params };
        const configuredCwd = typeof params.cwd === "string" ? params.cwd.trim() : "";
        if (!configuredCwd) {
          params.cwd = params.shell_kind === "wsl" ? "~" : await homeDir();
        } else {
          params.cwd = configuredCwd;
        }
      } catch (e) {
        setError(String(e));
        setConnecting(false);
        return;
      }
    }

    const pluginId = selectedMode; // "serial" | "ssh" | "tftp" | "telnet" | "iperf" | "network"
    // iperf 无单一连接目标（客户端目标每测可变、监听地址是配置项）——
    // endpoint 用字面量，默认名 `iperf @ iperf` 不携带易变的端口/版本
    // 网络调试：所有角色的 endpoint 统一带传输层前缀（tcp:// / udp://），
    // 使侧栏/状态栏/详情页自描述（网络会话状态栏无类型徽标，前缀即传输层标识）
    const endpoint = isSerial ? port : (isSsh ? sshHost : (isTftp ? `${tftpListenIp}:${tftpListenPort}` : (isIperf ? "iperf" : (isTelnet ? telnetHost : (isLocalShell ? String(params.cwd) : (isNetwork
      ? `${netTransport}://${netRole === "client"
          ? `${netRemoteHost}:${netRemotePort}`
          : `${netLocalHost || "0.0.0.0"}:${netLocalPort}`}`
      : selectedMode))))));
    // 网络调试默认会话名：带传输层与角色（"Network Debug @ TCP Client"），
    // 避免多个网络调试会话在左侧树里无法区分 server/client
    const networkDefaultName = `${pluginRegistry.get("network")?.manifest.name || "Network Debug"} @ ${netTransport.toUpperCase()} ${netRole === "server" ? "Server" : "Client"}`;
    // Telnet/TFTP/iperf/网络调试 无文件传输：不保存 transfer_protocol，避免无意义的 "ymodem" 默认值
    // 污染会话配置（传输能力由 effectiveTransferEnabled=false 表达）
    const effectiveTransferProtocol = isLocalShell || isTelnet || isTftp || isIperf || isNetwork ? undefined : transferProtocol;
    const effectiveSessionName = sessionName || undefined;

    try {
      if (editSessionId) {
        // 编辑模式：原地更新配置，保持 UUID 连续性
        await reconfigureSession(
          editSessionId,
          endpoint,
          params,
          effectiveSessionName,
          effectiveTransferEnabled,
          effectiveTransferProtocol,
          effectiveSendBarEnabled,
          undefined, // pluginId
          journaldEnabled,
        );
        onClose();
      } else {
        // 新建模式：仅保存配置，不连接（连接由右键菜单触发）
        const sid = await createOfflineSession(
          endpoint, params,
          isNetwork ? (sessionName || networkDefaultName) : effectiveSessionName, pluginId,
          effectiveTransferEnabled, effectiveTransferProtocol,
          effectiveSendBarEnabled,
        );
        if (sid) {
          await switchTab(sid);
          onClose();
        }
      }
    } catch (e) {
      setError(String(e));
    }
    setConnecting(false);
  }, [port, isSerial, isSsh, isTftp, isTelnet, isIperf, isNetwork, isLocalShell, PluginConnectForm, pluginParams, t, telnetHost, telnetPort, telnetSendBarEnabled, sshHost, tftpFileRoot, tftpListenIp, tftpListenPort, tftpWriteEnabled, tftpOverwrite, tftpSinglePort, baudRate, dataBits, parity, stopBits, flowControl, dataMode, encoding, dualFrameTimeout, transferEnabled, transferProtocol, sendBarEnabled, virtualPortEnabled, virtualPortCount, sessionName, selectedMode, editSessionId, createOfflineSession, reconfigureSession, switchTab, onClose, sshPort, sshUsername, sshAuthMethod, sshPassword, sshPrivateKey, sshPassphrase, sshSendBarEnabled, sshTransferEnabled, fileServiceEnabled, fileServiceProtocol, journaldEnabled, iperfVersion, iperfListenIp, iperfListenPort, netTransport, netRole, netRemoteHost, netRemotePort, netLocalHost, netLocalPort, netMaxClients, netConnectTimeoutMs, netNodelay, netBroadcast, netMulticastGroup, netTtl, netMulticastInterface, netSelfReceive]);

  // 数据字符编码下拉（终端类协议共用：serial / ssh / telnet）
  const encodingField = (
    <div className={styles.field}>
      <label className={styles.label}>{t("serial.encoding")}</label>
      <select
        className={`${styles.select} liquid-glass-input liquid-glass-select`}
        value={encoding}
        onChange={e => setEncoding(e.target.value)}
        disabled={connecting}
      >
        {CHARSETS.map(c => <option key={c.id} value={c.id}>{c.label}</option>)}
      </select>
    </div>
  );

  const handleOverlayClick = useCallback((e: React.MouseEvent) => {
    if (e.target === e.currentTarget) onClose();
  }, [onClose]);

  useEffect(() => {
    if (!isOpen) return;
    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    document.addEventListener("keydown", handleKeyDown);
    return () => document.removeEventListener("keydown", handleKeyDown);
  }, [isOpen, onClose]);

  return (
    <AnimatePresence>
      {isOpen && (
        <motion.div
          className={`${styles.overlay} glass-overlay`}
          initial={{ opacity: 0 }}
          animate={{ opacity: 1 }}
          exit={{ opacity: 0 }}
          transition={{ duration: 0.18, ease: [0.4, 0, 0.2, 1] }}
          onClick={handleOverlayClick}
        >
          <motion.div
            initial={{ y: 20, scale: 0.95, opacity: 0 }}
            animate={{ y: 0, scale: 1, opacity: 1 }}
            exit={{ y: 20, scale: 0.95, opacity: 0 }}
            transition={{ duration: 0.15, delay: 0.05, ease: [0.4, 0, 0.2, 1] }}
          >
            <div className={`${styles.dialog} liquid-glass`}>
          {/* ── 步骤 1: 模式选择（从 PluginRegistry 动态生成） ── */}
          {step === "mode" && (
            <>
              <h2 className={styles.title}>
                {editSessionId ? (t("contextMenu.reconnect") || "Reconnect") : t("session.newSession")}
              </h2>
              <p className={styles.subtitle}>{t("connectionType.label")}</p>
              <div className={styles.modeGrid}>
                {availableModes.map(mode => (
                  <motion.button
                    key={mode.id}
                    className={`${styles.modeCard} liquid-glass-card`}
                    whileHover={{ scale: 1.03, borderColor: "var(--accent-primary)" }}
                    whileTap={{ scale: 0.97 }}
                    onClick={() => handleModeSelect(mode.id)}
                  >
                    <Icon name={mode.icon} size="lg" className={styles.modeIcon} />
                    <span className={styles.modeLabel}>{mode.description}</span>
                  </motion.button>
                ))}
              </div>
              <div className={styles.actions}>
                <button className={`${styles.cancelBtn} liquid-glass-button`} onClick={onClose}>
                  {t("common.cancel")}
                </button>
              </div>
            </>
          )}

          {/* ── 步骤 2: 配置 ── */}
          {step === "config" && (
            <>
              <div className={styles.configHeader}>
                <button className={`${styles.backBtn} liquid-glass-button`} onClick={handleBack} disabled={connecting}>
                  <Icon name="arrow-left" size="sm" /> {t("common.back")}
                </button>
                <h2 className={styles.title}>
                  {(() => { const m = availableModes.find(m => m.id === selectedMode); return m ? <><Icon name={m.icon} size="md" />{" "}{m.description}</> : selectedMode; })()}
                </h2>
              </div>

              {/* 会话名称 */}
              <div className={styles.field}>
                <label className={styles.label}>{t("session.renameSession")} ({t("session.newSession")})</label>
                <input
                  className={`${styles.input} liquid-glass-input`}
                  type="text"
                  placeholder={isSerial ? port || "COM3" : (isLocalShell ? "Shell" : "My Session")}
                  value={sessionName}
                  onChange={e => setSessionName(e.target.value)}
                  disabled={connecting}
                />
              </div>

              {/* ── 串口配置 ── */}
              {isSerial && (
                <>
                  <div className={styles.field}>
                    <label className={styles.label}>{t("serial.port")}</label>
                    <div className={styles.row}>
                      <select className={`${styles.select} liquid-glass-input liquid-glass-select`} style={{ flex: 1 }} value={port} onChange={e => setPort(e.target.value)} disabled={connecting}>
                        {serialEndpoints.length === 0 && <option value="">{t("serial.noPorts")}</option>}
                        {serialEndpoints.map(ep => (
                          <option key={ep.name} value={ep.name}>{ep.name}{ep.description !== ep.name ? ` — ${ep.description}` : ""}</option>
                        ))}
                      </select>
                      <button className={`${styles.iconBtn} liquid-glass-button`} onClick={refreshEndpoints} title={t("serial.refresh")} disabled={connecting}><Icon name="refresh" size="md" /></button>
                    </div>
                  </div>

                  <div className={styles.row2}>
                    <div className={styles.field}>
                      <label className={styles.label}>{t("serial.baudRate")}</label>
                      <select className={`${styles.select} liquid-glass-input liquid-glass-select`} value={baudRate} onChange={e => setBaudRate(e.target.value)} disabled={connecting}>
                        {BAUD_RATES.map(b => <option key={b} value={b}>{b}</option>)}
                      </select>
                    </div>
                    <div className={styles.field}>
                      <label className={styles.label}>{t("serial.dataBits")}</label>
                      <select className={`${styles.select} liquid-glass-input liquid-glass-select`} value={dataBits} onChange={e => setDataBits(e.target.value)} disabled={connecting}>
                        {DATA_BITS.map(d => <option key={d} value={d}>{d}</option>)}
                      </select>
                    </div>
                  </div>

                  <div className={styles.row2}>
                    <div className={styles.field}>
                      <label className={styles.label}>{t("serial.parity")}</label>
                      <select className={`${styles.select} liquid-glass-input liquid-glass-select`} value={parity} onChange={e => setParity(e.target.value)} disabled={connecting}>
                        {PARITY.map(p => <option key={p.v} value={p.v}>{p.l}</option>)}
                      </select>
                    </div>
                    <div className={styles.field}>
                      <label className={styles.label}>{t("serial.stopBits")}</label>
                      <select className={`${styles.select} liquid-glass-input liquid-glass-select`} value={stopBits} onChange={e => setStopBits(e.target.value)} disabled={connecting}>
                        {STOP_BITS.map(s => <option key={s} value={s}>{s}</option>)}
                      </select>
                    </div>
                  </div>

                  <div className={styles.field}>
                    <label className={styles.label}>{t("serial.flowControl")}</label>
                    <select className={`${styles.select} liquid-glass-input liquid-glass-select`} value={flowControl} onChange={e => setFlowControl(e.target.value)} disabled={connecting}>
                      {FLOW_CONTROL.map(f => <option key={f.v} value={f.v}>{f.l}</option>)}
                    </select>
                  </div>

                  <div className={styles.field}>
                    <label className={styles.label}>{t("serial.dataMode")}</label>
                    <select className={`${styles.select} liquid-glass-input liquid-glass-select`} value={dataMode} onChange={e => setDataMode(e.target.value)} disabled={connecting}>
                      <option value="text">{t("serial.dataModeText")}</option>
                      <option value="hex">{t("serial.dataModeHex")}</option>
                      <option value="dual">{t("serial.dataModeDual")}</option>
                    </select>
                  </div>

                  {/* 数据字符编码（连接后不可变，改需重连） */}
                  {encodingField}

                  {/* Dual 模式分帧超时（仅 Dual 模式可见） */}
                  {dataMode === "dual" && (
                    <div className={styles.field}>
                      <label className={styles.label}>{t("serial.dualFrameTimeout")}</label>
                      <input
                        type="number"
                        className={`${styles.numberInput} liquid-glass-input`}
                        value={dualFrameTimeout}
                        min={5}
                        max={500}
                        step={5}
                        onChange={e => setDualFrameTimeout(Number(e.target.value))}
                        disabled={connecting}
                      />
                    </div>
                  )}

                  {/* 文件传输开关 */}
                  <div className={styles.field}>
                    <label className={`liquid-glass-toggle ${styles.checkboxLabel}`}>
                      <input
                        type="checkbox"
                        checked={transferEnabled}
                        onChange={e => setTransferEnabled(e.target.checked)}
                        disabled={connecting}
                      />
                      <div />
                      <span>{t("serial.enableTransfer")}</span>
                    </label>
                  </div>

                  {/* 传输协议选择（仅启用传输时可见） */}
                  {transferEnabled && (
                    <div className={styles.field}>
                      <label className={styles.label}>{t("serial.transferProtocol")}</label>
                      <select
                        className={`${styles.select} liquid-glass-input liquid-glass-select`}
                        value={transferProtocol}
                        onChange={e => setTransferProtocol(e.target.value as "ymodem" | "xmodem" | "zmodem")}
                        disabled={connecting}
                      >
                        <option value="ymodem">YModem</option>
                        <option value="xmodem">XModem</option>
                        <option value="zmodem">ZModem</option>
                      </select>
                    </div>
                  )}

                  {/* 发送栏开关 */}
                  <div className={styles.field}>
                    <label className={`liquid-glass-toggle ${styles.checkboxLabel}`}>
                      <input
                        type="checkbox"
                        checked={sendBarEnabled}
                        onChange={e => setSendBarEnabled(e.target.checked)}
                        disabled={connecting}
                      />
                      <div />
                      <span>{t("serial.enableSendBar") || "启用发送栏"}</span>
                    </label>
                  </div>

                  {/* 虚拟串口开关 */}
                  <div className={styles.field}>
                    <label className={`liquid-glass-toggle ${styles.checkboxLabel}`}>
                      <input
                        type="checkbox"
                        checked={virtualPortEnabled}
                        onChange={e => setVirtualPortEnabled(e.target.checked)}
                        disabled={connecting}
                      />
                      <div />
                      <span>{t("serial.enableVirtualPort") || "启用虚拟串口"}</span>
                    </label>
                  </div>

                  {/* 设备数量（仅启用虚拟串口时可见） */}
                  {virtualPortEnabled && (
                    <div className={styles.field}>
                      <label className={styles.label}>
                        {t("serial.virtualPortCount") || "设备数量"}
                      </label>
                      <select
                        className={`${styles.select} liquid-glass-input liquid-glass-select`}
                        value={virtualPortCount}
                        onChange={e => setVirtualPortCount(Number(e.target.value))}
                        disabled={connecting}
                      >
                        <option value={1}>1</option>
                        <option value={2}>2</option>
                        <option value={3}>3</option>
                        <option value={4}>4</option>
                      </select>
                    </div>
                  )}
                </>
              )}

              {/* ── SSH 配置 ── */}
              {isSsh && (
                <>
                  <div className={styles.field}>
                    <label className={styles.label}>{t("ssh.host")}</label>
                    <input
                      className={`${styles.input} liquid-glass-input`}
                      type="text"
                      placeholder={t("ssh.hostPlaceholder")}
                      value={sshHost}
                      onChange={e => setSshHost(e.target.value)}
                      disabled={connecting}
                    />
                  </div>

                  <div className={styles.row2}>
                    <div className={styles.field}>
                      <label className={styles.label}>{t("ssh.port")}</label>
                      <input
                        className={`${styles.numberInput} liquid-glass-input`}
                        type="number"
                        value={sshPort}
                        min={1}
                        max={65535}
                        onChange={e => {
                          const raw = e.target.value;
                          // 允许用户清空字段（中间编辑状态），重置为默认端口
                          if (raw === "") {
                            setSshPort(22);
                            return;
                          }
                          const n = Number(raw);
                          if (!isNaN(n) && n >= 1 && n <= 65535) {
                            setSshPort(n);
                          }
                          // 非法值忽略，保持当前状态（浏览器 type="number" 会阻止大部分非法输入）
                        }}
                        disabled={connecting}
                      />
                    </div>
                    <div className={styles.field}>
                      <label className={styles.label}>{t("ssh.username")}</label>
                      <input
                        className={`${styles.input} liquid-glass-input`}
                        type="text"
                        placeholder={t("ssh.usernamePlaceholder")}
                        value={sshUsername}
                        onChange={e => setSshUsername(e.target.value)}
                        disabled={connecting}
                      />
                    </div>
                  </div>

                  {/* 数据字符编码（连接后不可变，改需重连） */}
                  {encodingField}

                  <div className={styles.field}>
                    <label className={styles.label}>{t("ssh.authMethod")}</label>
                    <select
                      className={`${styles.select} liquid-glass-input liquid-glass-select`}
                      value={sshAuthMethod}
                      onChange={e => setSshAuthMethod(e.target.value as "password" | "key")}
                      disabled={connecting}
                    >
                      <option value="password">{t("ssh.authPassword")}</option>
                      <option value="key">{t("ssh.authKey")}</option>
                    </select>
                  </div>

                  {sshAuthMethod === "password" && (
                    <div className={styles.field}>
                      <label className={styles.label}>{t("ssh.password")}</label>
                      <input
                        className={`${styles.input} liquid-glass-input`}
                        type="password"
                        placeholder={t("ssh.passwordPlaceholder")}
                        value={sshPassword}
                        onChange={e => setSshPassword(e.target.value)}
                        disabled={connecting}
                      />
                    </div>
                  )}

                  {sshAuthMethod === "key" && (
                    <>
                      <div className={styles.field}>
                        <label className={styles.label}>{t("ssh.sshKey")}</label>
                        <textarea
                          className={`${styles.input} liquid-glass-input`}
                          rows={5}
                          placeholder={t("ssh.keyPlaceholder")}
                          value={sshPrivateKey}
                          onChange={e => setSshPrivateKey(e.target.value)}
                          disabled={connecting}
                          style={{ fontFamily: "var(--font-mono)", fontSize: "var(--text-xs)" }}
                        />
                      </div>
                      <div className={styles.field}>
                        <label className={styles.label}>{t("ssh.passphrase")}</label>
                        <input
                          className={`${styles.input} liquid-glass-input`}
                          type="password"
                          placeholder={t("ssh.passphrasePlaceholder")}
                          value={sshPassphrase}
                          onChange={e => setSshPassphrase(e.target.value)}
                          disabled={connecting}
                        />
                      </div>
                    </>
                  )}

                  {/* 文件服务协议固定为 SFTP（SCP 已移除） */}


                  {/* 启用发送栏开关 */}
                  <div className={styles.field}>
                    <label className={`liquid-glass-toggle ${styles.checkboxLabel}`}>
                      <input
                        type="checkbox"
                        checked={sshSendBarEnabled}
                        onChange={e => setSshSendBarEnabled(e.target.checked)}
                        disabled={connecting}
                      />
                      <div />
                      <span>{t("ssh.enableSendBar")}</span>
                    </label>
                  </div>

                  {/* 启用文件传输开关 */}
                  <div className={styles.field}>
                    <label className={`liquid-glass-toggle ${styles.checkboxLabel}`}>
                      <input
                        type="checkbox"
                        checked={sshTransferEnabled}
                        onChange={e => setSshTransferEnabled(e.target.checked)}
                        disabled={connecting}
                      />
                      <div />
                      <span>{t("ssh.enableTransfer")}</span>
                    </label>
                  </div>

                  {/* 启用文件管理器开关 */}
                  <div className={styles.field}>
                    <label className={`liquid-glass-toggle ${styles.checkboxLabel}`}>
                      <input
                        type="checkbox"
                        checked={fileServiceEnabled}
                        onChange={e => setFileServiceEnabled(e.target.checked)}
                        disabled={connecting}
                      />
                      <div />
                      <span>{t("ssh.enableFileService")}</span>
                    </label>
                  </div>

                  {/* 启用 journald 日志查看器开关 */}
                  <div className={styles.field}>
                    <label className={`liquid-glass-toggle ${styles.checkboxLabel}`}>
                      <input
                        type="checkbox"
                        checked={journaldEnabled}
                        onChange={e => setJournaldEnabled(e.target.checked)}
                        disabled={connecting}
                      />
                      <div />
                      <span>{t("journald.enableJournald")}</span>
                    </label>
                  </div>

                </>
              )}

              {/* ── TFTP 配置表单 ── */}
              {isTftp && (
                <>
                  <div className={styles.row2}>
                    <div className={styles.field}>
                      <label className={styles.label}>{t("tftp.listenIp")}</label>
                      <input
                        className={`${styles.input} liquid-glass-input`}
                        type="text"
                        value={tftpListenIp}
                        onChange={e => setTftpListenIp(e.target.value)}
                        disabled={connecting}
                        placeholder="0.0.0.0"
                      />
                    </div>
                    <div className={styles.field}>
                      <label className={styles.label}>{t("tftp.listenPort")}</label>
                      <input
                        className={`${styles.input} liquid-glass-input`}
                        type="number"
                        min={1}
                        max={65535}
                        value={tftpListenPort}
                        onChange={e => setTftpListenPort(Number(e.target.value))}
                        disabled={connecting}
                        placeholder="69"
                      />
                    </div>
                  </div>

                  <div className={styles.field}>
                    <label className={styles.label}>{t("tftp.fileRoot")}</label>
                    <div className={styles.row}>
                      <input
                        className={`${styles.input} liquid-glass-input`}
                        style={{ flex: 1 }}
                        type="text"
                        value={tftpFileRoot}
                        onChange={e => setTftpFileRoot(e.target.value)}
                        disabled={connecting}
                        placeholder="C:\tftp-root\"
                      />
                      <button
                        className={`${styles.iconBtn} liquid-glass-button`}
                        onClick={async () => {
                          const dir = await open({ directory: true, multiple: false });
                          if (dir && typeof dir === "string") setTftpFileRoot(dir);
                        }}
                        title={t("tftp.selectDir")}
                        disabled={connecting}
                      >
                        <Icon name="folder" size="md" />
                      </button>
                    </div>
                  </div>

                  <div className={styles.field}>
                    <label className={`liquid-glass-toggle ${styles.checkboxLabel}`}>
                      <input
                        type="checkbox"
                        checked={tftpWriteEnabled}
                        onChange={e => setTftpWriteEnabled(e.target.checked)}
                        disabled={connecting}
                      />
                      <div />
                      <span>{t("tftp.writeEnabled")}</span>
                    </label>
                  </div>

                  <div className={styles.field}>
                    <label className={`liquid-glass-toggle ${styles.checkboxLabel}`}>
                      <input
                        type="checkbox"
                        checked={tftpOverwrite}
                        onChange={e => setTftpOverwrite(e.target.checked)}
                        disabled={connecting}
                      />
                      <div />
                      <span>{t("tftp.overwrite")}</span>
                    </label>
                  </div>

                  <div className={styles.field}>
                    <label className={`liquid-glass-toggle ${styles.checkboxLabel}`}>
                      <input
                        type="checkbox"
                        checked={tftpSinglePort}
                        onChange={e => setTftpSinglePort(e.target.checked)}
                        disabled={connecting}
                      />
                      <div />
                      <span>{t("tftp.singlePort")}</span>
                    </label>
                  </div>
                </>
              )}

              {/* ── iperf 配置 ── */}
              {isIperf && (
                <>
                  <div className={styles.field}>
                    <label className={styles.label}>{t("iperf.version")}</label>
                    <select
                      className={`${styles.select} liquid-glass-input liquid-glass-select`}
                      value={iperfVersion}
                      onChange={e => {
                        const v = e.target.value as "iperf2" | "iperf3";
                        setIperfVersion(v);
                        // 端口联动（对齐版本切换规则：iperf2 默认 5001，iperf3 默认
                        // 5201）。仅当端口仍为默认值（用户未自定义）时切换默认；
                        // 自定义端口（如 9000）在来回切换版本后保留，不被静默覆盖
                        setIperfListenPort(prev =>
                          prev === 5001 || prev === 5201
                            ? (v === "iperf2" ? 5001 : 5201)
                            : prev
                        );
                      }}
                      disabled={connecting}
                    >
                      <option value="iperf2">iperf2</option>
                      <option value="iperf3">iperf3</option>
                    </select>
                  </div>
                  <div className={styles.row2}>
                    <div className={styles.field}>
                      <label className={styles.label}>{t("iperf.listenIp")}</label>
                      <input
                        className={`${styles.input} liquid-glass-input`}
                        type="text"
                        value={iperfListenIp}
                        onChange={e => setIperfListenIp(e.target.value)}
                        disabled={connecting}
                        placeholder="0.0.0.0"
                      />
                    </div>
                    <div className={styles.field}>
                      <label className={styles.label}>{t("iperf.listenPort")}</label>
                      <input
                        className={`${styles.input} liquid-glass-input`}
                        type="number"
                        min={1}
                        max={65535}
                        value={iperfListenPort}
                        onChange={e => {
                          // 空输入忽略（Number("") === 0 会污染保存的配置）
                          if (e.target.value === "") return;
                          const n = Number(e.target.value);
                          if (!Number.isInteger(n) || n < 1 || n > 65535) return;
                          setIperfListenPort(n);
                        }}
                        disabled={connecting}
                        placeholder="5001"
                      />
                    </div>
                  </div>
                </>
              )}

              {/* ── Telnet 配置 ── */}
              {isTelnet && (
                <>
                  <div className={styles.field}>
                    <label className={styles.label}>{t("telnet.host")}</label>
                    <input
                      className={`${styles.input} liquid-glass-input`}
                      type="text"
                      placeholder={t("telnet.hostPlaceholder")}
                      value={telnetHost}
                      onChange={e => setTelnetHost(e.target.value)}
                      disabled={connecting}
                    />
                  </div>

                  <div className={styles.field}>
                    <label className={styles.label}>{t("telnet.port")}</label>
                    <input
                      className={`${styles.numberInput} liquid-glass-input`}
                      type="number"
                      value={telnetPort}
                      min={1}
                      max={65535}
                      onChange={e => {
                        const raw = e.target.value;
                        // 允许用户清空字段（中间编辑状态），重置为默认端口
                        if (raw === "") {
                          setTelnetPort(23);
                          return;
                        }
                        const n = Number(raw);
                        if (!isNaN(n) && n >= 1 && n <= 65535) {
                          setTelnetPort(n);
                        }
                        // 非法值忽略，保持当前状态
                      }}
                      disabled={connecting}
                    />
                  </div>

                  {/* 数据字符编码（连接后不可变，改需重连） */}
                  {encodingField}

                  {/* 发送栏开关（默认开启） */}
                  <div className={styles.field}>
                    <label className={`liquid-glass-toggle ${styles.checkboxLabel}`}>
                      <input
                        type="checkbox"
                        checked={telnetSendBarEnabled}
                        onChange={e => setTelnetSendBarEnabled(e.target.checked)}
                        disabled={connecting}
                      />
                      <div />
                      <span>{t("telnet.enableSendBar") || "启用发送栏"}</span>
                    </label>
                  </div>
                </>
              )}

              {/* ── 网络调试配置（TCP/UDP 调试助手） ── */}
              {isNetwork && (
                <>
                  <div className={styles.field}>
                    <label className={styles.label}>{t("network.transport")}</label>
                    <select
                      className={`${styles.select} liquid-glass-input liquid-glass-select`}
                      value={netTransport}
                      onChange={e => setNetTransport(e.target.value as "tcp" | "udp")}
                      disabled={connecting}
                    >
                      <option value="tcp">{t("network.transportTcp")}</option>
                      <option value="udp">{t("network.transportUdp")}</option>
                    </select>
                  </div>

                  {/* TCP/UDP 均有 Client/Server 角色（UDP client = 固定远端单对端，UDP server = 绑本地多对端） */}
                  <div className={styles.field}>
                    <label className={styles.label}>{t("network.role")}</label>
                    <select
                      className={`${styles.select} liquid-glass-input liquid-glass-select`}
                      value={netRole}
                      onChange={e => setNetRole(e.target.value as "client" | "server")}
                      disabled={connecting}
                    >
                      <option value="client">{t("network.roleClient")}</option>
                      <option value="server">{t("network.roleServer")}</option>
                    </select>
                  </div>

                  {netRole === "client" && (
                    <>
                      <div className={styles.field}>
                        <label className={styles.label}>{t("network.remoteHost")}</label>
                        <input
                          className={`${styles.input} liquid-glass-input`}
                          type="text"
                          placeholder={t("network.remoteHostPlaceholder")}
                          value={netRemoteHost}
                          onChange={e => setNetRemoteHost(e.target.value)}
                          disabled={connecting}
                        />
                      </div>
                      <div className={styles.field}>
                        <label className={styles.label}>{t("network.remotePort")}</label>
                        <input
                          className={`${styles.numberInput} liquid-glass-input`}
                          type="number"
                          value={netRemotePort}
                          min={1}
                          max={65535}
                          onChange={e => {
                            const n = Number(e.target.value);
                            if (!isNaN(n)) setNetRemotePort(n);
                          }}
                          disabled={connecting}
                        />
                      </div>
                    </>
                  )}

                  {netRole === "server" && (
                    <>
                      <div className={styles.field}>
                        <label className={styles.label}>{t("network.localHost")}</label>
                        <input
                          className={`${styles.input} liquid-glass-input`}
                          type="text"
                          value={netLocalHost}
                          onChange={e => setNetLocalHost(e.target.value)}
                          disabled={connecting}
                        />
                      </div>
                      <div className={styles.field}>
                        <label className={styles.label}>{t("network.localPort")}</label>
                        <input
                          className={`${styles.numberInput} liquid-glass-input`}
                          type="number"
                          value={netLocalPort}
                          min={1}
                          max={65535}
                          onChange={e => {
                            const n = Number(e.target.value);
                            if (!isNaN(n)) setNetLocalPort(n);
                          }}
                          disabled={connecting}
                        />
                      </div>
                    </>
                  )}

                  {netTransport === "tcp" && netRole === "server" && (
                    <div className={styles.field}>
                      <label className={styles.label}>{t("network.maxClients")}</label>
                      <input
                        className={`${styles.numberInput} liquid-glass-input`}
                        type="number"
                        value={netMaxClients}
                        min={1}
                        max={1024}
                        onChange={e => {
                          const n = Number(e.target.value);
                          if (!isNaN(n)) setNetMaxClients(n);
                        }}
                        disabled={connecting}
                      />
                    </div>
                  )}

                  {netTransport === "tcp" && netRole === "client" && (
                    <div className={styles.field}>
                      <label className={styles.label}>{t("network.connectTimeoutMs")}</label>
                      <input
                        className={`${styles.numberInput} liquid-glass-input`}
                        type="number"
                        value={netConnectTimeoutMs}
                        min={100}
                        onChange={e => {
                          const n = Number(e.target.value);
                          if (!isNaN(n)) setNetConnectTimeoutMs(n);
                        }}
                        disabled={connecting}
                      />
                    </div>
                  )}

                  {netTransport === "udp" && netRole === "server" && (
                    <>
                      <div className={styles.field}>
                        <label className={`liquid-glass-toggle ${styles.checkboxLabel}`}>
                          <input
                            type="checkbox"
                            checked={netBroadcast}
                            onChange={e => setNetBroadcast(e.target.checked)}
                            disabled={connecting}
                          />
                          <div />
                          <span>{t("network.broadcast")}</span>
                        </label>
                      </div>
                      <div className={styles.field}>
                        <label className={styles.label}>{t("network.multicastGroup")}</label>
                        <input
                          className={`${styles.input} liquid-glass-input`}
                          type="text"
                          placeholder={t("network.multicastGroupPlaceholder")}
                          value={netMulticastGroup}
                          onChange={e => setNetMulticastGroup(e.target.value)}
                          disabled={connecting}
                        />
                        {/* IP_ADD_MEMBERSHIP 仅支持 IPv4 组播组（kernel IPv4-only），提前提示 */}
                        {netMulticastGroup && !/^2(2[4-9]|3\d)(\.\d{1,3}){3}$/.test(netMulticastGroup.trim()) && (
                          <div className={styles.hint}>{t("network.multicastIpv4Only")}</div>
                        )}
                      </div>
                      {netMulticastGroup && (
                        <>
                          <div className={styles.field}>
                            <label className={styles.label}>{t("network.ttl")}</label>
                            <input
                              className={`${styles.numberInput} liquid-glass-input`}
                              type="number"
                              value={netTtl}
                              min={1}
                              max={255}
                              onChange={e => {
                                const n = Number(e.target.value);
                                if (!isNaN(n)) setNetTtl(n);
                              }}
                              disabled={connecting}
                            />
                          </div>
                          <div className={styles.field}>
                            <label className={styles.label}>{t("network.multicastInterface")}</label>
                            <input
                              className={`${styles.input} liquid-glass-input`}
                              type="text"
                              value={netMulticastInterface}
                              onChange={e => setNetMulticastInterface(e.target.value)}
                              disabled={connecting}
                            />
                          </div>
                          <div className={styles.field}>
                            <label className={`liquid-glass-toggle ${styles.checkboxLabel}`}>
                              <input
                                type="checkbox"
                                checked={netSelfReceive}
                                onChange={e => setNetSelfReceive(e.target.checked)}
                                disabled={connecting}
                              />
                              <div />
                              <span>{t("network.selfReceive")}</span>
                            </label>
                          </div>
                        </>
                      )}
                    </>
                  )}

                  {/* 数据模式（连接后不可变，改需重连）：仅 TCP 流视图的 Dual/Text/Hex 渲染，UDP 恒为报文网格双栏 */}
                  {netTransport !== "udp" && (
                    <div className={styles.field}>
                      <label className={styles.label}>{t("serial.dataMode")}</label>
                      <select className={`${styles.select} liquid-glass-input liquid-glass-select`} value={dataMode} onChange={e => setDataMode(e.target.value)} disabled={connecting}>
                        <option value="text">{t("serial.dataModeText")}</option>
                        <option value="hex">{t("serial.dataModeHex")}</option>
                        <option value="dual">{t("serial.dataModeDual")}</option>
                      </select>
                    </div>
                  )}

                  {/* 数据字符编码（连接后不可变，改需重连） */}
                  {encodingField}
                </>
              )}

              {/* ── 未实现插件的占位提示 ── */}
              {PluginConnectForm && (
                <PluginConnectForm
                  params={pluginParams}
                  onChange={setPluginParams}
                  endpoints={state.endpoints.filter(endpoint => endpoint.connection_type === selectedMode)}
                />
              )}

              {!isSerial && !isSsh && !isTftp && !isTelnet && !isIperf && !isNetwork && !PluginConnectForm && (
                <div className={styles.comingSoonBanner} style={{ marginTop: 16 }}>
                  <Icon name="construction" size="lg" />{" "}
                  {t("connectionType.formNotImplemented", { pluginName: selectedMode })}
                </div>
              )}

              {error && <div className={styles.error}>{error}</div>}

              <div className={styles.actions}>
                <button className={`${styles.cancelBtn} liquid-glass-button`} onClick={onClose} disabled={connecting}>
                  {t("common.cancel")}
                </button>
                <button
                  className={`${styles.connectBtn} liquid-primary-button`}
                  onClick={handleCreate}
                  disabled={(!port && isSerial) || (!sshHost && isSsh) || (!tftpFileRoot && isTftp) || (!telnetHost && isTelnet) || (isNetwork && netRole === "client" && !netRemoteHost) || connecting}
                >
                  {connecting
                    ? t("serial.confirming")
                    : t("serial.confirm")}
                </button>
              </div>
            </>
          )}
        </div>
      </motion.div>
      </motion.div>
      )}
    </AnimatePresence>
  );
}
