# TRDP 权威知识索引

## IEC 61375-2-3

TRDP 属于 IEC 61375 列车通信网络标准体系。

- IEC Webstore 搜索入口: https://webstore.iec.ch/
- 标准编号：IEC 61375-2-3（Train Communication Network / Ethernet Consist Network 相关通信配置与 TRDP 语义）。

IEC 标准受版权保护且通常需要授权获取，TauTerm 仓库**不复制标准正文**。实现 PD、MD、拓扑、冗余或 SDT 语义时，应由有权访问标准的开发者核对对应版本。

## TCNOpen

TauTerm 当前 native TRDP 实现的直接工程基线是 **TCNOpen TRDP 3.0.0.0**。

- TCNOpen 官方项目: https://sourceforge.net/projects/tcnopen/
- TRDP release index: https://sourceforge.net/projects/tcnopen/files/TRDP/
- TauTerm vendored 来源记录: `src-tauri/vendor/tcnopen/SOURCE.json`
- TauTerm vendored MPL-2.0 license: `src-tauri/vendor/tcnopen/LICENSE`
- TauTerm downstream patch list: 同一 `SOURCE.json`

对 API、wire layout、返回码或 TCNOpen runtime 生命周期有疑问时，应优先检查**当前 vendored 3.0.0.0 代码**，而不是网上其他 TCNOpen 版本。

## TauTerm 的解释边界

- Node 是主动运行时；Monitor 是被动抓包/离线分析工作流。
- native request 必须以实际 ACK/Error 作为完成依据，不能把写入 helper stdin 当作成功。
- A/B Link 是网络路径；业务 redundancy group 是独立概念，不能自动等同。
- live/offline capture 应使用同一 Rust decoder 语义。
- SDT 元数据可以识别/保留，但 TauTerm 当前**不声称执行 SDTv2/SDTv4 安全语义验证、SIL 认证或安全认证**。

内部设计：[TRDP 模块设计](../modules/TRDP.md)。

## 测试材料

仓库中的 `samples/trdp/` 与 `tools/trdp-test-peer/` 是 TauTerm 自有测试/互通材料。它们用于验证实现，不是 IEC 标准正文的替代品。

当新增协议字段或状态机测试时，应说明依据来自 IEC 标准、TCNOpen 3.0.0.0 还是 TauTerm 自己的产品约束，避免三者混为一谈。
