# 网络调试模块设计

## 目标

网络模块提供通用网络连通、协议观察和吞吐测试能力，但共享 Session/Workspace 体验，不为每个工具重新发明一套生命周期。

当前模块族包括 Network Debug（TCP/UDP）、TFTP、Telnet 和 iperf。

## 当前方案

### Network Debug

TCP/UDP 使用统一的网络调试入口，但保持传输语义差异：

- TCP Client 是单连接流；TCP Server 可以管理多个 peer；
- UDP 是无连接 datagram，会保留报文边界和来源/目标信息；
- 发送目标由公共目标上下文表达，而不是把“广播”做成独立 UI 模式。

### TFTP

TFTP 是自包含 custom Session，文件传输和服务端控制在自己的视图完成，不使用全局 SendBar。对非本机监听并允许远端写入/覆盖的高风险组合，需要显式确认。

### Telnet

Telnet 是终端型 Session，协议协商在后端完成，UI 继续使用公共终端和发送能力。

### iperf

iperf 是自包含测试 Session，承载测试配置、运行过程、结果和服务端监听，不伪装成普通终端。

## 设计边界

- TCP stream 与 UDP datagram 的数据模型不能被强行统一到丢失边界信息的表示。
- Server peer 是运行时对象；Workspace 只保存稳定父配置。
- Network Debug 的公共发送目标应被手动发送和脚本共享，避免不同发送入口状态漂移。
- TFTP/iperf 这类 custom Session 的连接/配置/删除体验仍要遵守公共 Session 规则。
- 协议安全确认属于真正存在风险的操作边界，不靠普通提示文案替代。

## 代码锚点

- `src/plugins/network/`、`src-tauri/src/plugins/network/`
- `src/plugins/tftp/`、`src-tauri/src/plugins/tftp/`
- `src/plugins/telnet/`、`src-tauri/src/plugins/telnet/`
- `src/plugins/iperf/`、`src-tauri/src/plugins/iperf/`
- `src/components/Network/`、`src/components/Tftp/`、`src/components/Iperf/`

## 何时更新本文

修改 TCP peer 模型、UDP datagram 模型、目标选择、TFTP 风险边界、Telnet 协商职责、iperf 生命周期或这些模块与公共 Session 的关系时，必须同步更新本文。
