---
name: tauterm-docs
description: >
  Maintain TauTerm's documentation system — README.md (English, user-facing), README.zh-CN.md (Chinese mirror), docs/ARCHITECTURE.md, docs/BUILDING.md, CHANGELOG.md. Use this skill whenever making ANY code change to the TauTerm project (new features, plugin/protocol changes, commands, UI, config, build or driver changes) — the change is NOT complete until the docs are synced. Also use when the user asks to update, fix, or check documentation consistency, or mentions the roadmap, protocol matrix, or CHANGELOG, even if they don't explicitly say "docs".
license: MIT
metadata:
  author: tauterm
  version: "1.0"
---

# TauTerm 文档维护

## 为什么重要

README 是获客入口（营销文档），docs/ 是开发者文档，CHANGELOG 是发布记录。改了代码不改文档 = 卖点与现实脱节，用户第一分钟就会发现。双语文档漂移会让国际用户或国内用户看到过期内容。

## 文档地图

先判断改动类型，再决定改哪些文件：

| 改动类型 | 需要更新的文档 |
|---|---|
| 新功能 / 用户可见行为变化 | README.md + README.zh-CN.md（放进正确人群分组）+ CHANGELOG.md |
| 协议插件 / 内核 / 架构变化 | docs/ARCHITECTURE.md + README 协议支持矩阵与功能列表同步 |
| 构建 / 环境 / 驱动 / 安装变化 | docs/BUILDING.md（影响终端用户时同步 README Quick Install 章节） |
| Roadmap 状态变化（完成 / 新增里程碑） | 两版 README 的 Roadmap 章节同步 |
| 纯内部重构（无用户可见变化） | 不更新用户文档；必要时仅 CHANGELOG.md |

## 铁律

1. **双语文档镜像**：README 的任何改动必须同时应用到 README.md 与 README.zh-CN.md，章节标题一一对应。中文版按中文习惯表达语义一致的内容（不是逐字翻译），顶部保留 "Chinese mirror of the English README" 说明。
2. **禁拉踩**：两版 README 不得出现竞品名（MobaXterm / WindTerm / VOFA+ / Tabby）或负面对比表述（如与 Electron 系应用比较资源占用、提及其他软件停更等）。只描述 TauTerm 自身的优势。这是用户的明确要求。
3. **语言分工**：README 两版是营销文档，面向用户、保持精简（≤ ~350 行），不放架构细节；docs/ 是开发者文档（中文）；CONTRIBUTING.md 与 CHANGELOG.md 是英文。
4. **CHANGELOG**：Keep a Changelog 格式、英文、按类别分条目（如 ### Protocols / ### Terminal Engine）；未发布的改动进入下一版本段。
5. **链接有效**：所有相对链接必须指向存在的文件；移动章节后同步修正引用它的链接。
6. **不编造数据**：性能表实测数据、截图（TODO 占位）不得虚构；没有实测就标注待测，不要填编出来的数字。

## 完成标准

任何文档更新后必须运行：

```bash
node scripts/check-docs.js
```

全绿才算完成；若脚本报错，逐条修复后重跑。

## 注意事项

- 新增功能条目放进 README 正确的人群分组（For Network Engineers / For Embedded Developers / For Everyone），不要塞进错误的分组。
- 协议矩阵中状态变化（📋 计划中 → ✅ 已实现）时，同时更新 CHANGELOG 与 Roadmap。
- 改动范围要克制：与本次改动无关的文档内容不要顺手"优化"，避免噪音 diff。
