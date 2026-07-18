/**
 * SSH 插件前端注册
 *
 * 向内核注册 SSH 协议插件的 manifest、翻译资源。
 * 连接表单在 ConnectDialog 中内联渲染（与串口表单相同的模式）。
 */
import { registerPlugin } from "../../core/plugin-registry";

registerPlugin({
  manifest: {
    id: "ssh",
    name: "SSH",
    version: "1.0.0",
    category: "terminal",
    description: "SSH 远程终端",
    icon: "ssh-shell",
    content_type: "terminal",
    capabilities: ["connection", "transfer", "endpoint_discovery"],
    transfer_protocols: ["sftp"],
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
