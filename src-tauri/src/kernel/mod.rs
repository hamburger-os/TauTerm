//! TauTerm 微内核模块
//!
//! 12 个内核模块提供平台能力，不包含任何协议实现或业务 UI 组件。
//! 所有会话类型（Serial、SSH、Telnet 等）均作为插件注册到 Plugin Host。
//!
//! ## 模块
//!
//! - `config_store`    — 类型安全 KV 存储，JSON Schema 校验，命名空间隔离
//! - `ipc_bridge`      — Tauri 命令动态注册、类型事件总线、Stream 通道
//! - `tab_host`        — 标签页 CRUD、会话关联、生命周期事件
//! - `plugin_host`     — 插件发现/加载/初始化/停止全生命周期
//! - `plugin_adapter`  — ProtocolAdapter trait + ContentType/IoStrategy 定义
//! - `session_store`   — 会话存储、I/O 生命周期、统计采集
//! - `shortcut_engine` — 全局/插件作用域快捷键注册、冲突检测、作用域分发
//! - `theme_engine`    — CSS 变量生成、运行时主题切换、插件 token 注入
//! - `i18n_engine`     — 命名空间隔离翻译、插件翻译资源注册、动态语言切换
//! - `window_manager`  — 窗口创建/关闭、布局持久化、分屏状态管理
//! - `log_engine`      — 生产者-消费者异步日志引擎，系统事件 + 会话数据日志
//! - `log_writer`      — 单日志文件写入器，格式化（text/hex/dual）、自动分卷

pub mod config_store;
pub mod i18n_engine;
pub mod ipc_bridge;
pub mod log_engine;
pub mod log_writer;
pub mod plugin_adapter;
pub mod plugin_host;
pub mod session_store;
pub mod shortcut_engine;
pub mod tab_host;
pub mod theme_engine;
pub mod window_manager;
