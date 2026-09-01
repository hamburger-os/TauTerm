# TauTerm 文档 / Documentation

本目录保存 TauTerm 的长期产品、工程与发布文档。

This directory contains TauTerm's long-lived product, engineering and release documentation.

根目录 [README](../README.md) 是面向社区和用户的产品入口。`docs/` 中的文档应各自承担一个明确职责，并明确区分“已经实现的能力”和“未来方向”。

The root [README](../README.md) is the public product entry point. Documents under `docs/` should each have one clear responsibility and should distinguish shipped behavior from future direction.

## 产品方向 / Product Direction

这一组文档主要用于产品决策和长期规划，**以中文为主**。其中描述的规划能力不构成版本交付承诺。

These documents primarily support product decisions and long-term planning and are **Chinese-first**. Planned capabilities described here are not release commitments.

| 文档 / Document | 用途 / Purpose |
|---|---|
| [PRODUCT_STRATEGY.md](PRODUCT_STRATEGY.md) | 产品愿景、核心用户、产品原则、战略能力、路线图模型与产品决策过滤器 / Product vision, users, principles, capability pillars and roadmap model |
| [HARDWARE_ECOSYSTEM.md](HARDWARE_ECOSYSTEM.md) | 自研分析仪统一上位机方向、仪器数据模型、时间体系与未来 CAN 集成 / First-party instrument platform, data/timing model and future CAN integration |
| [COMMERCIALIZATION.md](COMMERCIALIZATION.md) | 商业分层、授权原则、硬件业务、工业模块与商业验证门槛 / Commercial packaging, licensing, hardware business and validation gates |

## 工程与开发 / Engineering & Development

这一组文档面向贡献者和工程实现，当前**以英文为主**，后续可以根据社区需要补充双语版本。

These documents describe the current codebase, build environment and supported platforms. They are currently **English-first** and may become bilingual where useful.

| 文档 / Document | 用途 / Purpose |
|---|---|
| [ARCHITECTURE.md](ARCHITECTURE.md) | 当前微内核、协议 Adapter、Session/I/O 与前后端架构 / Current microkernel, protocol adapters, session/I/O and frontend/backend architecture |
| [BUILDING.md](BUILDING.md) | 开发环境、依赖与源码构建流程 / Developer prerequisites and source-build instructions |
| [SUPPORTED_PLATFORMS.md](SUPPORTED_PLATFORMS.md) | 支持的系统、架构、打包与签名状态 / Supported operating systems, architectures, packaging and signing status |

当某个未来产品概念真正实现为稳定子系统后，应把技术契约补充到架构/工程文档，而不是只保留在战略文档中。

When a future product concept becomes a real subsystem, its technical contract should move into architecture/engineering documentation instead of remaining only in strategy documents.

## 发布文档 / Release Documentation

发布相关文档主要面向维护者和社区，保持英文或双语均可。

Release documentation is maintainer/community-facing and may remain English or bilingual.

| 文档 / Document | 用途 / Purpose |
|---|---|
| [RELEASING.md](RELEASING.md) | 维护者发布流程与 Release Engineering / Maintainer release process and release engineering |
| [RELEASE_NOTES_v0.5.0.md](RELEASE_NOTES_v0.5.0.md) | v0.5.0 历史 Release Notes |
| [RELEASE_NOTES_v0.5.1.md](RELEASE_NOTES_v0.5.1.md) | v0.5.1 历史 Release Notes |

根目录 [CHANGELOG.md](../CHANGELOG.md) 继续作为已经发布变化的权威时间线记录。

The repository root [CHANGELOG.md](../CHANGELOG.md) remains the canonical chronological record of shipped changes.

## 资源 / Assets

`assets/` 保存 README 和长期文档引用的截图与其他媒体资源。描述已经实现的功能时，应优先使用真实应用输出，而不是 Mockup。

`assets/` contains durable screenshots and media referenced by repository documentation. Real application output should be preferred over mockups when documenting shipped features.

## 文档原则 / Documentation Principles

新增或修改文档时遵循：

1. **一个文档，一个职责。** 如果已有权威文档负责该主题，不重复创建新文件。
2. **只描述 TauTerm 自己。** 直接说明问题、设计与目标工作流，不使用具名竞品比较或拉踩式表达。
3. **严格区分现状与方向。** 用户/架构文档不得把规划能力写成已经实现。
4. **按能力组织产品方向。** 优先描述工程结果与共享平台能力，而不是堆功能清单。
5. **明确文档边界和交叉链接。** 产品、架构、硬件、商业化之间存在边界时应显式互链。
6. **只保存长期知识。** 临时推广文案、一次性发布宣传和短期 Campaign Checklist 不放入长期工程文档。
7. **新增长期文档必须更新本索引。** 保持 `docs/` 可导航。
8. **语言服务于读者。** 给产品决策者看的内部方向文档以中文为主；面向国际社区、贡献者和开发者的文档可以使用英文或双语。

When adding or revising documentation:

1. **One document, one responsibility.** Do not duplicate a topic already owned by a canonical document.
2. **Describe TauTerm on its own terms.** Explain the problem, design and intended workflow directly; avoid named product comparisons.
3. **Separate current state from direction.** Planned capabilities must not be presented as shipped behavior.
4. **Prefer capability-oriented structure.** Product direction should focus on engineering outcomes and shared platform capabilities.
5. **Keep boundaries and cross-links explicit.** Product, architecture, hardware and commercialization documents should reference each other where responsibilities meet.
6. **Keep durable knowledge here.** Temporary promotion and one-off campaign material should live elsewhere.
7. **Update this index for new durable documents.** Keep `docs/` navigable.
8. **Choose language for the audience.** Internal strategy is Chinese-first; international community and contributor documentation may be English or bilingual.

## 权威信息顺序 / Source of Truth

如果不同文档出现不一致，按以下顺序判断：

1. **已经发布的行为：** 当前代码、测试、[CHANGELOG.md](../CHANGELOG.md) 与 Release 产物；
2. **当前技术设计：** [ARCHITECTURE.md](ARCHITECTURE.md) 与其他工程文档；
3. **未来产品方向：** [PRODUCT_STRATEGY.md](PRODUCT_STRATEGY.md) 与相关战略文档。

If documents disagree, use this priority:

1. **Shipped behavior:** current code, tests, [CHANGELOG.md](../CHANGELOG.md) and release artifacts;
2. **Current technical design:** [ARCHITECTURE.md](ARCHITECTURE.md) and engineering documentation;
3. **Future product direction:** [PRODUCT_STRATEGY.md](PRODUCT_STRATEGY.md) and related strategy documents.

战略文档可以指导实现，但不能作为某项能力“已经发布”的证据。

Strategy documents guide implementation, but they are not evidence that a capability has shipped.
