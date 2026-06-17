# architecture-polish (delta)

## ADDED Requirements

### Requirement: Architecture stub types are documented as reserved
`channel/mod.rs` 中标记 `#[allow(dead_code)]` 的 `IoStrategy` 和 `ContentType` 枚举 SHALL 移除 `#[allow(dead_code)]` 属性，添加文档注释说明其作为多协议架构桩的预留用途，使每个变体通过文档即可理解其设计意图。

#### Scenario: IoStrategy is documented without dead_code suppression
- **WHEN** 开发者阅读 `channel/mod.rs` 中的 `IoStrategy` 枚举
- **THEN** 枚举上方 SHALL 包含 `/// 预留: 用于区分同步/异步 I/O 策略，当前仅使用 Sync 变体（串口），Async 变体为 SSH/TCP 插件预留` 文档注释
- **AND** 枚举定义中不存在 `#[allow(dead_code)]` 注解
- **AND** `cargo build` SHALL 无相关 dead_code 警告（因 pub 导出被视为"已使用"）

#### Scenario: ContentType is documented without dead_code suppression
- **WHEN** 开发者阅读 `channel/mod.rs` 中的 `ContentType` 枚举
- **THEN** 枚举上方 SHALL 包含文档注释说明各变体对应的前端渲染器（Terminal → TerminalRenderer、FileBrowser → FileBrowserRenderer、StatsDashboard → StatsDashboardRenderer、Custom → CustomRenderer）
- **AND** 枚举定义中不存在 `#[allow(dead_code)]` 注解
