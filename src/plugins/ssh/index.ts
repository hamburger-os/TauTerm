/**
 * SSH 插件前端注册
 *
 * SSH 专属连接身份、持久化参数清洗与展示摘要均由插件拥有；公共 Session/UI
 * 只消费声明式 contribution，不解析 SSH 字段。
 */
import { registerPlugin, type PluginManifest } from "../../core/plugin-registry";
import manifestJson from "../../plugin-manifests/ssh.json";

function formatHostPort(host: string, port: number): string {
  const trimmedHost = host.trim();
  const displayHost = trimmedHost.includes(":")
    && !(trimmedHost.startsWith("[") && trimmedHost.endsWith("]"))
    ? `[${trimmedHost}]`
    : trimmedHost;
  return `${displayHost}:${port}`;
}

registerPlugin({
  manifest: manifestJson as PluginManifest,
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
      "selectKey": "选择密钥...",
      "enableSendBar": "启用发送栏",
      "enableTransfer": "启用文件传输",
      "enableFileService": "启用文件管理器",
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
      "keyPlaceholder": "-----BEGIN OPENSSH PRIVATE KEY-----",
      "selectKey": "Select key...",
      "enableSendBar": "Enable Send Bar",
      "enableTransfer": "Enable File Transfer",
      "enableFileService": "Enable File Manager",
      "connect": "Connect",
      "confirm": "Confirm",
      "confirming": "Connecting...",
    },
  },
});

console.log("[Plugin] SSH plugin registered");
