# 网络协议权威知识索引

## TCP / UDP

### 权威来源

- TCP — RFC 9293: https://www.rfc-editor.org/rfc/rfc9293
- UDP — RFC 768: https://www.rfc-editor.org/rfc/rfc768
- IPv4 multicast / IGMP 等行为应按对应 IETF RFC 与操作系统 socket API 共同核对。

### TauTerm 适用范围

Network Debug 的 TCP 是可靠字节流；UDP 是保留 datagram 边界的无连接消息。实现不能把 TCP 的“连接/流”语义直接套到 UDP，也不能丢失 UDP 的单报文来源信息。

内部设计：[NETWORK.md](../modules/NETWORK.md)。

## SSH

### 权威来源

- SSH Architecture — RFC 4251: https://www.rfc-editor.org/rfc/rfc4251
- SSH Authentication — RFC 4252: https://www.rfc-editor.org/rfc/rfc4252
- SSH Transport Layer — RFC 4253: https://www.rfc-editor.org/rfc/rfc4253
- SSH Connection Protocol — RFC 4254: https://www.rfc-editor.org/rfc/rfc4254

TauTerm 的多终端 child channel 语义应以 SSH Connection Protocol 的 channel 模型和当前 `russh` 行为共同校验。

## SFTP

SFTP 没有最终发布为 IETF RFC。TauTerm 当前依赖 `russh-sftp`，其实现目标是 SFTP v3。

- `russh-sftp` 官方 crate 文档: https://docs.rs/russh-sftp/
- Historical SFTP v3 draft: https://datatracker.ietf.org/doc/html/draft-ietf-secsh-filexfer-02

因此文档中不要把“SFTP v3”写成“RFC 标准”。需要实现扩展时，应优先确认 `russh-sftp` 实际支持范围。

内部设计：[SSH.md](../modules/SSH.md)、[TRANSFER.md](../modules/TRANSFER.md)。

## Telnet

### 权威来源

- Base Telnet — RFC 854: https://www.rfc-editor.org/rfc/rfc854
- Telnet Option Specifications — RFC 855: https://www.rfc-editor.org/rfc/rfc855
- Binary Transmission — RFC 856: https://www.rfc-editor.org/rfc/rfc856
- Echo — RFC 857: https://www.rfc-editor.org/rfc/rfc857
- Suppress Go Ahead — RFC 858: https://www.rfc-editor.org/rfc/rfc858
- NAWS — RFC 1073: https://www.rfc-editor.org/rfc/rfc1073

TauTerm 的 option negotiation、local echo 和窗口尺寸更新需要同时满足协议状态机与当前 telnet crate 的能力。

## TFTP

### 权威来源

- TFTP Revision 2 — RFC 1350: https://www.rfc-editor.org/rfc/rfc1350
- TFTP Option Extension — RFC 2347: https://www.rfc-editor.org/rfc/rfc2347
- Blocksize Option — RFC 2348: https://www.rfc-editor.org/rfc/rfc2348
- Timeout Interval / Transfer Size — RFC 2349: https://www.rfc-editor.org/rfc/rfc2349
- Windowsize Option — RFC 7440: https://www.rfc-editor.org/rfc/rfc7440

当前 `tftpd 1.0` 上游声明覆盖 RFC 1350、2347、2348、2349 和 7440；TauTerm 的具体 UI/配置只应声称自身实际暴露和验证过的子集。安全暴露策略（监听非 loopback、远端写入、覆盖）是 TauTerm 产品安全边界，不是 TFTP RFC 本身提供的权限系统。

## iperf

iperf2/iperf3 是工具协议实现，不是同一个 IETF wire standard，也不能假设互通。

- iperf3 官方 ESnet 文档: https://software.es.net/iperf/
- iperf2 官方项目: https://sourceforge.net/projects/iperf2/
- TauTerm vendored riperf3 上游: https://github.com/therealevanhenry/riperf3

TauTerm 的 iperf3 行为还必须参考 `src-tauri/vendor/riperf3/VENDOR-NOTES.md`，因为当前代码是有下游修改的 vendored fork。

## Modbus

### 权威来源

- Modbus Organization Specifications: https://www.modbus.org/modbus-specifications
- Modbus Application Protocol V1.1b3 由上述官方页面发布。
- Modbus Serial Line Protocol and Implementation Guide V1.02 由上述官方页面发布。

TauTerm 当前 Protocol Inspector 覆盖 Modbus RTU、ASCII 与 TCP 的离线 ADU/PDU 检查，并对常用 01/02/03/04/05/06/0F/10 功能做字段和语义验证。RTU/ASCII 的串行 framing/checksum 与 TCP 的 MBAP 必须保持各自标准语义。该能力不是在线 Modbus stack、设备模拟器或一致性认证工具；未来 Modbus Session 应复用同一协议核心，而不是复制 parser/function table。

内部设计：[OBSERVABILITY_TOOLS.md](../modules/OBSERVABILITY_TOOLS.md)。


## NMEA 0183

### 权威来源

- NMEA 0183 官方标准入口: https://www.nmea.org/nmea-0183.html

NMEA 0183 标准正文受 NMEA 发布和许可约束；仓库只保存权威入口与 TauTerm 的适用边界，不复制标准正文。当前 Protocol Inspector 只提供常见 ASCII sentence framing、talker/type/字段拆分、XOR checksum，以及少量常用 GGA/RMC 字段的通用解释，不声明覆盖所有 sentence、厂商扩展或 NMEA 合规认证。

内部设计：[OBSERVABILITY_TOOLS.md](../modules/OBSERVABILITY_TOOLS.md)。
