# 嵌入式调试权威索引

> 本文只索引 TauTerm 嵌入式调试能力实现时需要核对的上游权威资料，不复制第三方文档内容，也不作为 TauTerm 当前功能清单。

## 适用范围

当前主要服务 RTT 调试助手，包括调试探针发现、SWD/JTAG attach、目标内存中的 RTT Control Block、Up/Down Channel 语义，以及与已有 J-Link 调试会话共存的 RTT TELNET 路径。以后若加入 SWO/ITM、Flash、SVD、Memory/Register 或其它 Debug Probe 能力，应继续在这里补充对应一手资料。

## probe-rs

- probe-rs 项目与源代码：https://github.com/probe-rs/probe-rs
- 官方文档入口：https://probe.rs/docs/
- Debug Probe 设置与平台说明：https://probe.rs/docs/getting-started/probe-setup/
- Rust API 文档：https://docs.rs/probe-rs/
- RTT API：https://docs.rs/probe-rs/latest/probe_rs/rtt/

实现 native probe backend 前应以 TauTerm 锁定的 `probe-rs` 版本 API 为准，不根据旧博客或 CLI 输出猜测 library contract。重点核对 `Lister`/probe selector、WireProtocol、target attach、Session/Core 生命周期、RTT `ScanRegion`、Up/Down Channel read/write 和错误类型。

`probe-rs` 的许可证及传递依赖许可证由 Cargo metadata 与仓库许可证检查共同验证。升级版本必须重新运行 Cargo license gate，并重新核对平台 probe/driver 要求。

## SEGGER RTT / J-Link

- SEGGER RTT Knowledge Base：https://kb.segger.com/RTT
- J-Link RTT TELNET Channel：https://kb.segger.com/J-Link_RTT_TELNET_Channel
- J-Link RTT Viewer：https://www.segger.com/products/debug-probes/j-link/tools/rtt-viewer/
- J-Link RTT Client：https://kb.segger.com/J-Link_RTT_Client
- J-Link SDK：https://www.segger.com/products/debug-probes/j-link/tools/j-link-sdk/

TauTerm 当前只把 J-Link RTT TELNET/Existing Debug Session 作为兼容 backend；它不是 J-Link SDK 集成，也不意味着可以重新分发 SEGGER SDK。若以后评估 SDK，必须先重新审查许可证、分发权和平台边界。

Existing Session backend 仅连接本机 loopback 服务，不能因为底层是 TCP 就扩展成未经设计的远程服务入口。Channel/Control Block 等能力应按该接口实际可提供的能力降级，不能把 native probe backend 的 introspection 能力套用过去。RTT TELNET Channel 选择必须遵循 SEGGER 官方 Config String 契约，在连接建立后的协议窗口内发送完整配置串，不能把 `RTTCh` 子命令当作裸文本命令发送。

## SEGGER SystemView

- SystemView target source：https://github.com/SEGGERMicro/SystemView
- SystemView target implementation：https://github.com/SEGGERMicro/SystemView/blob/main/SYSVIEW/SEGGER_SYSVIEW.c
- SystemView public API/event IDs：https://github.com/SEGGERMicro/SystemView/blob/main/SYSVIEW/SEGGER_SYSVIEW.h
- SystemView host command IDs：https://github.com/SEGGERMicro/SystemView/blob/main/SYSVIEW/SEGGER_SYSVIEW_Int.h
- Upstream license：https://github.com/SEGGERMicro/SystemView/blob/main/LICENSE.md

TauTerm 的 SystemView decoder 以目标端公开源代码中的 packet framing、event ID、变长整数、timestamp delta 和 Down Channel command 定义为协议依据；不从第三方 UI 截图反推二进制格式。上游 source 许可证允许在保留条件下再分发/修改，但 TauTerm 当前只实现兼容 decoder/control path，不复制或 vendoring 上游实现，因此不存在额外 bundled-source 许可证文件。

SystemView 数据使用目标端 timestamp delta 建立时间线；host acquisition timestamp 只用于采集诊断，不能替代 target time。SystemView control 与普通 SendBar 写入属于不同语义：START/STOP/metadata 请求由 semantic observer 内部路径发送，公共发送不得写入被 observer claim 的 Down Channel。

## 设计核对原则

- RTT Up 与 Down Channel 分别核对，不假设同 index 必然构成双向 stream。
- Control Block 查找失败与 probe/target attach 失败保持不同错误语义。
- Target memory/debug-probe 所有权保持单一，避免多个线程或多个 Session runtime 同时操作同一底层 handle。
- 第三方 library/driver 的平台限制属于 backend 边界；公共 Session/Workspace 不编码厂商或探针型号判断。
- 任何发送、reset、flash、replay 等可能改变真实目标状态的未来能力，都必须单独审查安全交互，不从只读/低影响 RTT 监控语义自动继承权限。
