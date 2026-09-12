# 网络调试模块设计

## 目标

网络模块提供通用网络连通、协议观察和吞吐测试能力，但共享 Session/Workspace/Transport Runtime，不为每个工具重新实现生命周期或底层 socket 所有权。

当前模块族包括 Network Debug（TCP/UDP）、TFTP、Telnet 和 iperf。

## 当前方案

### Network Debug

TCP/UDP 使用统一的网络调试入口，但保持传输语义差异：

- TCP Client 是单连接流；TCP Server 可以管理多个 peer；每个 TCP peer 由独立 `DataPlaneRuntime` 驱动并作为 Session 子连接注册；
- UDP 是无连接 datagram，使用共享 `UdpTransport`，保留报文边界和来源/目标地址；
- Network 根 Session 持有一个 aggregate DataPlane，负责把 TCP peer/UDP 的接收数据镜像给脚本和自动回复，而地址感知 UI 事件仍由 Network 模块产生；
- TCP peer 的普通写入走各自 `SessionIo`，TCP server 的选中 peer/全部 peer 路由由 Network 的明确目标语义处理；
- 发送目标由公共目标上下文表达，而不是把“广播”做成独立 UI 模式；
- 目标选择可以在未连接会话中保留，但只有父 Session 已连接后才同步到后端运行时。

TCP connect/listen 使用 `transport::tcp`，UDP bind/recv/send 使用 `transport::udp`。Network 协议层不再维护自己的通用 TCP channel 或第二套 I/O loop。

### TFTP

TFTP 是自包含 custom Session，文件传输和服务端控制在自己的视图完成，不使用全局 SendBar。服务端默认关闭远程写入和覆盖；用户主动开启“允许写入”后才可配置覆盖。对“非回环监听 + 允许写入 + 允许覆盖”的组合，配置页显示非阻塞行内风险提示，但不再追加二次确认弹窗或后端确认令牌。

### Telnet

Telnet 是终端型 Session。Telnet 协商与 IAC/NAWS 语义留在插件 driver 中，底层 TCP 与 Session 生命周期继续复用 Transport/DataPlane；UI 使用公共终端和发送能力。

### iperf

iperf 是自包含测试 Session，承载测试配置、运行过程、结果和服务端监听，不伪装成普通终端。

## 设计边界

- TCP stream 与 UDP datagram 的数据模型不能被强行统一到丢失报文边界/地址信息的表示。
- Server peer 是运行时对象；Saved Session 只保存稳定父配置。
- TCP peer 使用公共 DataPlane/SessionIo 生命周期；父监听器关闭时级联清理 peer，单 peer 关闭不反向关闭监听器。
- Network aggregate DataPlane 只为 Session 级脚本/自动回复提供统一接收与发送语义；来源地址、peer ID 等协议视图信息仍归 Network 模块。
- Network Debug 的目标选择必须被手动发送和脚本共享；目标同步属于运行时副作用，只能发生在已连接 Network Debug 会话。
- TFTP/iperf custom Session 的连接/配置/删除仍遵守公共 Session 规则。
- TFTP 的保护策略以保守默认值和显式开关为主；高风险组合需要清楚可见的行内 warning，但不阻塞专业调试流程。

## 代码锚点

- `src/plugins/network/`
- `src-tauri/src/plugins/network/mod.rs`
- `src-tauri/src/transport/tcp.rs`
- `src-tauri/src/transport/udp.rs`
- `src-tauri/src/transport/runtime.rs`
- `src-tauri/src/kernel/session_store.rs`
- `src/plugins/tftp/`
- `src-tauri/src/plugins/tftp/`
- `src/plugins/telnet/`
- `src-tauri/src/plugins/telnet/`
- `src/plugins/iperf/`
- `src-tauri/src/plugins/iperf/`
- `src/components/Network/`
- `src/components/Tftp/`
- `src/components/Iperf/`

## 何时更新本文

修改 TCP peer 模型、UDP datagram 模型、aggregate DataPlane、目标选择、TFTP 风险边界、Telnet 协商职责、iperf 生命周期或这些模块与公共 Session 的关系时，必须同步更新本文。

Transport 语义见 [TRANSPORT_RUNTIME.md](TRANSPORT_RUNTIME.md)。TCP/UDP、Telnet、TFTP、iperf 相关权威依据索引在 [NETWORK_PROTOCOLS.md](../knowledge/NETWORK_PROTOCOLS.md)；Modbus 单独见 [MODBUS.md](MODBUS.md)。