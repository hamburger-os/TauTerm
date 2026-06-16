# project-scaffold

## Purpose

定义 TauTerm 项目的基础脚手架要求，包括项目初始化、构建系统、依赖管理和文档。

## Requirements

### Requirement: Tauri v2 项目初始化
项目必须被初始化为 Tauri v2 应用，包含 Rust 后端和 React + TypeScript 前端。

#### Scenario: 项目构建成功
- **WHEN** 开发者在项目根目录运行 `npm install && cargo build`
- **THEN** 项目必须编译无错误，并生成可运行的 Tauri 二进制文件

#### Scenario: 开发服务器启动
- **WHEN** 开发者运行 `npm run tauri dev`
- **THEN** 应用窗口必须打开，显示 TauTerm 界面

### Requirement: Git 配置
项目必须包含 `.gitignore` 文件，配置忽略 Rust、Node.js、Tauri 和 IDE 产生的临时文件。

#### Scenario: Git 忽略构建产物
- **WHEN** 构建后运行 `git status`
- **THEN** `target/`、`node_modules/`、`dist/` 目录不得显示为未跟踪文件

### Requirement: README 说明文档
项目必须包含 `README.md` 文件，描述项目目的、技术栈、构建说明和功能概述，文档使用中文编写。

#### Scenario: README 包含构建说明
- **WHEN** 新贡献者阅读 README
- **THEN** 他们必须能找到在 Windows、Linux 和 macOS 上安装依赖和运行项目的清晰步骤

### Requirement: 依赖管理
Rust 后端必须声明依赖 `tauri` v2、`serde`、`serde_json`、`serialport`、`tokio` 和 `thiserror`。前端必须声明依赖 `react`、`react-dom`、`@tauri-apps/api`、`@xterm/xterm`、`i18next` 和 `react-i18next`。

#### Scenario: Cargo.toml 正确配置
- **WHEN** 在 `src-tauri/` 中执行 `cargo build`
- **THEN** 所有 Rust 依赖必须解析成功，后端必须编译通过

#### Scenario: package.json 正确配置
- **WHEN** 在项目根目录执行 `npm install`
- **THEN** 所有前端依赖必须安装无错误

### Requirement: 多语言基础设施
项目必须初始化 i18n 国际化框架，创建 `src/i18n/` 目录结构，包含中文（默认）和英文语言资源文件。

#### Scenario: i18n 目录结构就绪
- **WHEN** 项目脚手架搭建完成后
- **THEN** `src/i18n/locales/zh-CN.json` 和 `src/i18n/locales/en-US.json` 必须存在，包含基础 UI 文本的键值对

#### Scenario: i18n 配置初始化
- **WHEN** 应用启动
- **THEN** i18next 必须被正确初始化，默认语言设置为 `zh-CN`，并支持回退到 `zh-CN`
