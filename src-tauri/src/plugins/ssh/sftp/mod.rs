//! SSH SFTP 侧通道。
//!
//! 目录结构即所有权：SFTP 是 SSH 的能力，因此能力对象、文件服务、批量传输和
//! 远程文档都归 SSH 插件；通用 `transfer` 模块只保留协议无关的 `FileTransfer`
//! 抽象、传输编排与串口协议。
//!
//! - `capability` — SFTP 子系统会话的唯一 owner（惰性打开、失效重建、home 解析）
//! - `service` — 目录浏览、属性、增删改、上传/下载的有界实现
//! - `transfer` — `FileTransfer` trait 的 SFTP 实现（复用 TransferScheduler）
//! - `document` — 远程文档读取/保存事务
//! - `error` — `SftpError` 分类（上层唯一依赖的错误模型）
//! - `paths` / `attrs` — 远端路径与 POSIX mode 的单一实现

pub mod attrs;
pub mod capability;
pub mod document;
pub mod error;
pub mod paths;
pub mod service;
pub mod transfer;
