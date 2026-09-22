/** SSH frontend plugin definition. */
import { createElement } from "react";
import { definePlugin, type PluginManifest } from "../../core/plugin-registry";
import manifestJson from "../../plugin-manifests/ssh.json";
import SshConnectForm, {
  DEFAULT_SSH_PARAMS,
  isSshConnectionConfigValid,
  normalizeSshParams,
} from "./SshConnectForm";
import SshStatusItems from "./SshStatusItems";
import SshHostKeyGate from "./SshHostKeyGate";
import { SshFileManagerSidebarPanel, SshJournaldSidebarPanel } from "./SshRightSidebarPanels";

function formatHostPort(host: string, port: number): string {
  const trimmedHost = host.trim();
  const displayHost = trimmedHost.includes(":")
    && !(trimmedHost.startsWith("[") && trimmedHost.endsWith("]"))
    ? `[${trimmedHost}]`
    : trimmedHost;
  return `${displayHost}:${port}`;
}

const connected = ({ activeTab }: { activeTab: { state: string } | null }) =>
  activeTab?.state === "connected" || activeTab?.state === "transferring";

export const sshPlugin = definePlugin({
  manifest: manifestJson as PluginManifest,
  connectForm: SshConnectForm,
  defaultConnectionParams: () => ({ ...DEFAULT_SSH_PARAMS }),
  defaultSessionOptions: () => ({ transferEnabled: false, sendBarEnabled: false }),
  normalizeConnectionParams: normalizeSshParams,
  isConnectionConfigValid: isSshConnectionConfigValid,
  resolveEndpoint: params => String(params.host ?? "").trim(),
  persistedConnectionParams: (params, sessionId) => {
    const persisted = { ...params };
    delete persisted.password;
    delete persisted.private_key;
    delete persisted.passphrase;
    persisted.credential_account = `ssh-session:${sessionId}`;
    return persisted;
  },
  sessionPresentation: {
    defaultName: params => {
      const username = typeof params.username === "string" && params.username.trim()
        ? params.username.trim()
        : "root";
      return `SSH @ ${username}`;
    },
    subtitle: (params, endpoint) => {
      const host = typeof params.host === "string" && params.host.trim()
        ? params.host.trim()
        : endpoint;
      const port = typeof params.port === "number" && Number.isInteger(params.port)
        && params.port > 0 && params.port <= 65535
        ? params.port
        : 22;
      return formatHostPort(host, port);
    },
  },
  appOverlay: SshHostKeyGate,
  rightSidebar: {
    panels: [
      {
        id: "ssh-file-manager",
        when: params => params.file_service_enabled === true,
        component: SshFileManagerSidebarPanel,
      },
      {
        id: "ssh-journald",
        when: params => params.journald_enabled === true,
        component: SshJournaldSidebarPanel,
      },
    ],
  },
  statusBarItems: [
    {
      id: "ssh-runtime",
      priority: 860,
      when: connected,
      render: context => createElement(SshStatusItems, context),
    },
  ],
  locales: {
    "zh-CN": {
      "host": "主机地址",
      "port": "端口",
      "username": "用户名",
      "authMethod": "认证方式",
      "authPassword": "密码",
      "authKey": "SSH 密钥",
      "password": "密码",
      "sshKey": "SSH 私钥",
      "passphrase": "密码短语",
      "passphrasePlaceholder": "（无密码短语）",
      "hostPlaceholder": "192.168.1.1",
      "usernamePlaceholder": "root",
      "passwordPlaceholder": "输入密码",
      "keyPlaceholder": "-----BEGIN OPENSSH PRIVATE KEY-----",
      "selectKey": "选择密钥...",
      "enableSendBar": "启用发送栏",
      "enableTransfer": "启用文件传输",
      "enableFileService": "启用文件管理器",
      "hostKeyTitle": "SSH 主机密钥验证",
      "hostKeyChangedTitle": "SSH 主机身份已变化",
      "hostKeyFirstSeenPrompt": "主机：{{host}}\n算法：{{algorithm}}\n指纹：{{fingerprint}}\n\n这是首次看到该主机身份。请通过可信渠道核对指纹后再确认。",
      "hostKeyAdditionalPrompt": "主机：{{host}}\n算法：{{algorithm}}\n指纹：{{fingerprint}}\n已信任算法：{{knownAlgorithms}}\n\n该主机已经有受信任身份，但本次协商出现了新的主机密钥算法。请核对设备身份后再新增信任。",
      "hostKeyChangedPrompt": "主机：{{host}}\n算法：{{algorithm}}\n已信任指纹：\n{{expected}}\n当前指纹：{{actual}}\n\n连接已被拒绝。只有在确认设备确实更换了主机密钥后才应更新信任；确认后需要重新连接。",
      "hostKeyTrustUpdated": "SSH 主机信任已更新，请重新连接。",
      "hostKeyError": "SSH 主机密钥操作失败：{{error}}",
      "connect": "连接",
      "confirm": "确认",
      "confirming": "连接中...",
    },
    "en-US": {
      "host": "Host",
      "port": "Port",
      "username": "Username",
      "authMethod": "Auth Method",
      "authPassword": "Password",
      "authKey": "SSH Key",
      "password": "Password",
      "sshKey": "SSH Private Key",
      "passphrase": "Passphrase",
      "passphrasePlaceholder": "(no passphrase)",
      "hostPlaceholder": "192.168.1.1",
      "usernamePlaceholder": "root",
      "passwordPlaceholder": "••••••••",
      "keyPlaceholder": "-----BEGIN OPENSSH PRIVATE KEY-----",
      "selectKey": "Select key...",
      "enableSendBar": "Enable Send Bar",
      "enableTransfer": "Enable File Transfer",
      "enableFileService": "Enable File Manager",
      "hostKeyTitle": "SSH Host Key Verification",
      "hostKeyChangedTitle": "SSH Host Identity Changed",
      "hostKeyFirstSeenPrompt": "Host: {{host}}\nAlgorithm: {{algorithm}}\nFingerprint: {{fingerprint}}\n\nThis is the first observed identity for this host. Verify the fingerprint through a trusted channel before confirming.",
      "hostKeyAdditionalPrompt": "Host: {{host}}\nAlgorithm: {{algorithm}}\nFingerprint: {{fingerprint}}\nTrusted algorithms: {{knownAlgorithms}}\n\nThis host already has a trusted identity, but negotiation presented a new host-key algorithm. Verify the device identity before adding trust.",
      "hostKeyChangedPrompt": "Host: {{host}}\nAlgorithm: {{algorithm}}\nTrusted fingerprint(s):\n{{expected}}\nCurrent fingerprint: {{actual}}\n\nThe connection was refused. Update trust only after confirming that the device really changed its host key; reconnect afterward.",
      "hostKeyTrustUpdated": "SSH host trust was updated. Reconnect to continue.",
      "hostKeyError": "SSH host-key operation failed: {{error}}",
      "connect": "Connect",
      "confirm": "Confirm",
      "confirming": "Connecting...",
    },
  },
});
