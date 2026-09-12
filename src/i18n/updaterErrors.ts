import type { UpdaterErrorKind } from "../utils/updaterError";

type UpdaterErrorMessages = Record<UpdaterErrorKind, string>;

/**
 * Localized updater failure vocabulary.
 *
 * Kept as one small, focused source because updater failures are runtime
 * categories shared by the updater hook and diagnostics rather than page copy.
 */
export const updaterErrorTranslations: Record<
  "zh-CN" | "en-US",
  UpdaterErrorMessages
> = {
  "zh-CN": {
    timeout: "更新服务响应超时，请稍后重试。详细信息已写入日志。",
    dns: "无法解析更新服务器地址，请检查 DNS 或网络设置。详细信息已写入日志。",
    proxy: "代理连接更新服务失败，请检查系统或网络代理设置。详细信息已写入日志。",
    tls: "与更新服务建立安全连接失败，请检查系统证书、TLS 或网络安全软件。详细信息已写入日志。",
    http: "更新服务器返回了异常 HTTP 响应，请稍后重试。详细信息已写入日志。",
    metadata: "更新元数据无效或与当前平台不匹配。详细信息已写入日志。",
    signature: "更新签名验证失败。为保护更新安全，TauTerm 已停止安装。详细信息已写入日志。",
    transport: "无法连接更新服务，请检查网络、代理或 TLS/证书设置。详细信息已写入日志。",
    install: "更新下载或安装失败。为避免损坏安装，TauTerm 已停止更新。详细信息已写入日志。",
    relaunch: "更新已准备完成，但 TauTerm 无法自动重新启动。请手动重新启动应用。详细信息已写入日志。",
    unknown: "检查更新失败，请稍后重试。详细信息已写入日志。",
  },
  "en-US": {
    timeout: "The update service timed out. Please try again later. Details were written to the log.",
    dns: "The update server address could not be resolved. Check DNS or network settings. Details were written to the log.",
    proxy: "The update service could not be reached through the proxy. Check system or network proxy settings. Details were written to the log.",
    tls: "A secure connection to the update service could not be established. Check system certificates, TLS, or network security software. Details were written to the log.",
    http: "The update server returned an unexpected HTTP response. Please try again later. Details were written to the log.",
    metadata: "The update metadata is invalid or does not match this platform. Details were written to the log.",
    signature: "Update signature verification failed. TauTerm stopped the installation to preserve update security. Details were written to the log.",
    transport: "The update service could not be reached. Check the network, proxy, or TLS/certificate settings. Details were written to the log.",
    install: "The update could not be downloaded or installed. TauTerm stopped the update to avoid damaging the installation. Details were written to the log.",
    relaunch: "The update is ready, but TauTerm could not restart automatically. Restart the application manually. Details were written to the log.",
    unknown: "The update check failed. Please try again later. Details were written to the log.",
  },
};
