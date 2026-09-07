# TauTerm 维护者开发手册

> 这份文档主要给项目维护者使用。它回答“平时开发时应该让 AI 做什么、我应该看什么、常用命令是什么”。命令本身的可执行定义仍以根目录 `package.json` 为唯一来源，本表只做中文导航。

## 1. 日常开发方式

推荐把一次开发任务分成四步：

1. **先设计**：让 AI 先阅读 `AGENTS.md`、对应 `docs/modules/*.md` 和相关 `docs/knowledge/*.md`，明确方案与标准依据。
2. **再实现**：代码和对应模块设计文档在同一个变更中完成。
3. **再验证**：执行与改动匹配的检查；涉及跨平台、native helper 或权限边界时不能只跑前端检查。
4. **最后审查文档**：你优先审查 `docs/README.md` 与被修改的模块文档，确认架构、数据流、边界与实现意图一致。

如果 AI 只修改代码，没有更新被影响的模块文档，应视为任务未完成。

## 2. 常用命令速查

| 命令 | 什么时候用 |
|---|---|
| `npm run tauri dev` | **最常用开发入口。** 构建 TRDP native helper 后启动完整桌面应用 |
| `npm run dev` | 只需要 Vite 前端时使用；不代表完整 TauTerm |
| `npm run build` | TypeScript 检查并生成前端生产资源 |
| `npm run preview` | 本地预览已经生成的前端资源 |
| `npm run docs:check` | 检查文档层级、链接、资产引用、README 对齐等 |
| `npm run license:check` | 检查第三方许可证、来源记录和分发 notice 契约 |
| `npm run license:cargo` | 读取完整 Cargo dependency metadata，检查缺失或需人工复核的许可证表达式 |
| `npm run trdp:build` | 单独诊断 TRDP native helper / vendored TCNOpen 构建 |
| `npm run tauri:build` | Windows 开发打包配置下生成 NSIS 安装包 |
| `npm run build:release` | 正式发布前执行完整 release build |
| `npm run toolchain:check` | 检查 Rust 工具链是否与仓库锁定值一致 |
| `npm run version:check` | 检查各处版本号是否一致 |
| `npm run release:check -- X.Y.Z` | 发布前检查指定版本与 CHANGELOG |
| `npm run version:sync` | 从 `package.json` 同步 Cargo/Tauri 版本元数据 |
| `npm run check-com0com` | Windows com0com 分发文件与合规文件完整性检查 |
| `npm run check-reserved-region` | 检查虚拟串口测试保留区与产品常量一致 |
| `npm run check:brand-neutral-ui` | 检查产品 UI 与主题文案的品牌中性约束 |
| `npm run check:product-integrity` | 检查插件单一来源、工程资产持久化、日志可观测性、SSH 主机信任与 Pane UI 结构合同 |
| `npm run check:split-layout` | Workspace/Split View 几何、持久化与 1/2/2×2 Pane 不变量回归 |
| `npm run check:session-buffer` | 会话启动数据缓冲回归 |
| `npm run check:icons -- --strict` | 图标资产严格检查 |
| `npm run prompt:icon -- <key>` | 按图标语义规范生成提示词 |
| `npm run preview:icons` | 生成图标小尺寸/主题预览 |
| `npx tsc --noEmit` | 仅做 TypeScript 类型检查 |
| `cargo fmt --manifest-path src-tauri/Cargo.toml -- --check` | Rust 格式检查 |
| `cargo clippy --locked --all-targets --no-deps --manifest-path src-tauri/Cargo.toml -- -D warnings` | Rust 严格静态检查 |
| `cargo test --locked --manifest-path src-tauri/Cargo.toml` | Rust 测试 |

新增、删除或重命名 npm script 时，应同步调整本速查表，但不要在这里复制 script 的实际命令字符串。

## 3. 我应该看哪些文档

| 情况 | 优先审查 |
|---|---|
| 总体架构改变 | `docs/README.md` |
| 某个功能/协议改变 | 对应 `docs/modules/*.md` |
| 是否符合协议/平台标准 | 对应 `docs/knowledge/*.md` |
| 产品方向改变 | `docs/product/*.md` |
| 构建/平台/发布流程改变 | `docs/community/*.md` |
| 安全漏洞报告方式 | `SECURITY.md` |
| 第三方许可与来源 | `THIRD_PARTY_LICENSES.md` |
| 已完成版本变化 | `CHANGELOG.md` |

## 4. 开发任务的文档验收问题

审查一次重要改动时，可以只问下面几件事：

- 这个改动属于哪个模块，模块职责有没有变？
- 数据从哪里进入、由谁拥有、在哪里释放？
- 持久化状态与运行时状态有没有混在一起？
- 是否新增权限、native helper、第三方库或外部标准依赖？
- 是否改变用户能看到的工作流？
- 对应模块文档有没有同步？
- 对应知识文档中的权威依据是否仍正确？
- 是否增加了需要长期维护的新重复事实？

## 5. 发布前

发布流程以 [社区发布文档](../community/RELEASING.md) 为准。维护者只需要特别确认：

1. `CHANGELOG.md` 已准确描述本版本；
2. `npm run docs:check` 和 `npm run license:check` 通过；
3. 版本/工具链检查通过；
4. CI 三平台全部通过；
5. 第三方源码、许可证和二进制分发方式没有在本版本中发生未记录变化。
