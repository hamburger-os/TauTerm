# TauTerm 标准知识库

> 本目录保存 TauTerm 开发时需要查阅的**外部权威知识索引**。它不是 TauTerm 当前实现说明，也不是把标准正文复制进仓库。

## 1. 作用

设计文档回答“TauTerm 现在怎么做”；知识文档回答“这个领域的权威依据是什么”。

开发协议、终端、平台安全、第三方许可证等功能前，AI 应先阅读对应知识文档，再查其中列出的官方来源。遇到内部设计与标准/上游行为冲突时，不能自行猜测，应回到权威来源确认并修订设计。

## 2. 来源优先级

知识材料按下面顺序选择：

1. **正式标准机构**：RFC Editor / IETF、IEC、ECMA、IEEE、TIA 等；
2. **平台/协议官方组织**：操作系统厂商文档、Modbus Organization、TCNOpen 官方发布；
3. **项目上游官方文档**：Tauri、Lua、xterm.js、实际依赖库的官方仓库/文档；
4. **社区文章/博客**：仅用于补充背景，不作为规范性实现依据。

如果正式标准与某个实际依赖库行为不完全一致，应同时记录“标准要求”和“当前库实现边界”。

## 3. 版权规则

- 不把付费/受版权保护的 IEC、TIA、IEEE 等标准全文复制进仓库。
- 只记录标准编号、标题、官方入口、适用章节/概念和 TauTerm 使用边界。
- RFC、ECMA 或开源项目允许公开阅读，也优先链接原始发布页面而不是维护本地副本。
- 如果为测试保存样例，样例必须是 TauTerm 自己构造或许可证允许再分发的材料。

## 4. 知识索引

| 文档 | 覆盖范围 |
|---|---|
| [NETWORK_PROTOCOLS.md](NETWORK_PROTOCOLS.md) | TCP/UDP、SSH/SFTP、Telnet、TFTP、iperf、Modbus |
| [TERMINAL_SERIAL_AUTOMATION.md](TERMINAL_SERIAL_AUTOMATION.md) | 终端控制序列、PTY/ConPTY、串口标准、Lua |
| [TRDP.md](TRDP.md) | IEC 61375-2-3 / TRDP、TCNOpen、SDT 边界 |
| [PLATFORM_SECURITY.md](PLATFORM_SECURITY.md) | Tauri 安全模型、权限、updater、平台 native 边界 |
| [LICENSE_COMPLIANCE.md](LICENSE_COMPLIANCE.md) | TauTerm 双许可证、GPL/MPL、第三方再分发检查 |

## 5. 维护规则

每份知识文档应包含：

- 权威来源；
- TauTerm 为什么需要它；
- 当前实现采用的版本/范围；
- 明确的非目标或风险边界；
- 相关内部模块文档。

当协议标准、实际依赖版本或平台 API 发生变化时，先更新知识文档中的来源/适用性，再决定是否修改 TauTerm 设计。

知识层**不记录“已实现/未实现”的版本历史**；那仍然属于 `CHANGELOG.md` 和模块设计文档。
