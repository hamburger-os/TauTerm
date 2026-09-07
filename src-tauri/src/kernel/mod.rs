//! TauTerm 微内核模块
//!
//! 当前内核模块提供平台能力，不包含任何协议实现或业务 UI 组件。
//! 所有会话类型（Serial、SSH、Telnet 等）均作为插件注册到 Plugin Host。
//!
//! ## 模块
//!
//! - `config_store`    — 版本化非敏感 KV/工程资产存储，命名空间隔离与磁盘持久化
//! - `plugin_host`     — canonical PluginManifest 的运行时注册与能力查询
//! - `plugin_adapter`  — ProtocolAdapter trait + ContentType/IoStrategy 定义
//! - `file_transfer`   — 统一文件传输 trait（FileTransfer）+ 进度/取消抽象
//! - `session_store`   — 会话存储、I/O 生命周期、统计采集
//! - `theme_engine`    — CSS 变量生成、运行时主题切换、插件 token 注入
//! - `log_engine`      — 生产者-消费者异步日志引擎，系统事件 + 会话数据日志
//! - `log_writer`      — 单日志文件写入器，格式化（text/hex/dual）、自动分卷
//! - `charset`         — 字符编码转码（UTF-8 ↔ GBK/Big5/Shift-JIS 等，发送转码 + 日志解码）

pub mod charset;
pub mod comm_handle;
pub mod config_store;
pub mod data_batcher;
pub mod file_transfer;
pub mod log_engine;
pub mod log_writer;
pub mod plugin_adapter;
pub mod plugin_host;
pub mod script_engine;
pub mod session_store;
pub mod theme_engine;
