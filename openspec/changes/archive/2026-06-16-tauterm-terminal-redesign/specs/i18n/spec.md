# i18n (Delta)

## MODIFIED Requirements

### Requirement: 多语言支持
系统必须支持多语言国际化，默认语言为简体中文（`zh-CN`），并允许用户切换至英文（`en-US`）。翻译文件必须扩展以覆盖新增的会话管理、搜索、命令面板和快捷键功能。

#### Scenario: 应用默认语言
- **WHEN** 应用首次启动
- **THEN** 所有 UI 文本必须以简体中文显示，包括新增的标签页、搜索、命令面板文本

#### Scenario: 切换至英文
- **WHEN** 用户在设置中切换语言
- **THEN** 所有 UI 文本必须即时切换为英文，无需重启应用

### Requirement: 语言文件结构
语言资源必须以 JSON 格式存储，扩展命名空间以覆盖新增功能模块。

#### Scenario: 中文语言文件完整性
- **WHEN** 查看 `zh-CN.json`
- **THEN** 文件必须包含 `app`、`serial`、`transfer`、`settings`、`common`、`session`、`search`、`palette`、`shortcuts` 和 `theme` 命名空间

#### Scenario: 英文语言文件完整性
- **WHEN** 查看 `en-US.json`
- **THEN** 文件必须包含与中文语言文件完全相同的键结构

## ADDED Requirements

### Requirement: 新增命名空间翻译
系统必须为以下新增功能模块提供完整的中英文翻译：

#### Scenario: Session 命名空间
- **WHEN** 会话相关 UI 渲染
- **THEN** 必须包含新会话、关闭会话、重命名、会话列表、无会话、确认关闭等文本

#### Scenario: Search 命名空间
- **WHEN** 搜索栏渲染
- **THEN** 必须包含搜索占位符、匹配计数（如"第 1 个，共 5 个"）、无结果、大小写敏感切换

#### Scenario: Command Palette 命名空间
- **WHEN** 命令面板打开
- **THEN** 必须包含占位符、无命令匹配、分类标题（会话、终端、传输、主题、应用）

#### Scenario: Shortcuts 命名空间
- **WHEN** 快捷键显示
- **THEN** 必须包含所有默认快捷键的描述文本

#### Scenario: Theme 命名空间
- **WHEN** 主题选择 UI 渲染
- **THEN** 必须包含主题名称（Neon Dark、Ocean Blue、Sunset Amber）和切换操作文本
