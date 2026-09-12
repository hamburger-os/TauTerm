# 平台与安全权威知识索引

## Tauri v2

### 权威来源

- Tauri Security: https://v2.tauri.app/security/
- Capabilities / permissions: https://v2.tauri.app/security/capabilities/
- Updater plugin: https://v2.tauri.app/plugin/updater/
- Windows Code Signing: https://v2.tauri.app/distribute/sign/windows/
- Configuration: https://v2.tauri.app/reference/config/

TauTerm 的 IPC 能力、CSP、资源打包、sidecar/updater 等设计必须与当前 Tauri v2 文档一致。平台专属配置覆盖规则也要按 Tauri 配置合并语义核对，不能假设数组/资源一定自动合并。

## Windows 权限与服务

涉及以下能力时，应使用 Microsoft Learn 的对应 Win32/Service/Named Pipe 文档：

- Windows Services: https://learn.microsoft.com/windows/win32/services/services
- Named Pipes: https://learn.microsoft.com/windows/win32/ipc/named-pipes
- Process creation/elevation: https://learn.microsoft.com/windows/win32/api/shellapi/nf-shellapi-shellexecuteexw
- Pseudoconsole: 见 [TERMINAL_SERIAL_AUTOMATION.md](TERMINAL_SERIAL_AUTOMATION.md)

TauTerm 的长期原则是主 GUI 保持普通权限，只把必要动作放到窄特权边界。平台 API 允许做某事不等于 TauTerm 应扩大权限范围。

## 凭据与加密

TauTerm 当前凭据存储设计由 [PLATFORM_SECURITY 模块文档](../modules/PLATFORM_SECURITY.md) 负责。密码派生和 AEAD 算法实现应依据所使用 Rust crates 的当前官方文档与相应算法规范，不在知识库复制密码学细节。

高风险调整（KDF 参数、密钥生命周期、vault 格式、认证失败处理）必须同时检查：

- 当前依赖库官方文档；
- persisted format 向后/向前兼容边界；
- 跨平台 contract tests；
- 不在日志中泄露秘密。

## 软件更新

Tauri updater 使用签名验证。TauTerm 自己的 release workflow 还会验证最终资产集合、签名和 public download 后再把稳定版本提升为 updater latest。

更新网络故障不得通过关闭证书校验、接受无效主机名或允许不安全传输来规避。Tauri updater 支持请求超时、代理和请求头；涉及网络兼容性时应先保留完整错误链并区分 transport、DNS、proxy、TLS、HTTP、metadata、signature 与 install 阶段，再针对有证据的故障调整实现。任何 fallback 都必须继续保持签名验证和 HTTPS fail-closed 边界。

Updater 的 Rust core 与 JavaScript bindings 属于同一发布基础设施边界。升级时应同步验证两侧解析版本、Windows 安装生命周期和真实 updater asset，不允许依赖宽泛版本范围在构建时静默漂移。

外部机制依据以 Tauri updater 文档为准；TauTerm 自己额外的 fail-closed 发布流程属于 [PLATFORM_SECURITY.md](../modules/PLATFORM_SECURITY.md) 与 [RELEASING.md](../community/RELEASING.md)。

## 第三方 native 组件

com0com、TCNOpen、Npcap/libpcap 等组件同时涉及平台安全与许可证。来源/许可证权威索引见 [LICENSE_COMPLIANCE.md](LICENSE_COMPLIANCE.md) 和根目录 `THIRD_PARTY_LICENSES.md`。
