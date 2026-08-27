//! VirtualPortBackend trait — 虚拟端点后端抽象接口
//!
//! 按“能力”抽象虚拟串口，而不是把 Windows 的 COM 端口对模型泄漏到上层。
//! 当前实现：Windows 使用 com0com；Linux/macOS 使用进程内 POSIX PTY。

use serde::{Deserialize, Serialize};

/// 一个由 TauTerm 管理并暴露给外部工具的虚拟端点。
///
/// `bridge_path` 仅供桥接层定位内部侧；`external_path` 是外部程序应打开的路径；
/// `resource_id` 是后端不透明资源标识，不承诺具有端口号或 bus 语义。
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct VirtualEndpoint {
    pub bridge_path: String,
    pub external_path: String,
    pub resource_id: u32,
}

/// 用于创建虚拟端点的配置。
#[derive(Debug, Clone)]
pub struct VirtualPortConfig {
    pub enabled: bool,
    pub count: u32,
}

/// 统一权限不足检测 — 同时用于 `Err(String)`（spawn 失败）和
/// `Ok(Output)`（setupc.exe 启动成功但内核驱动拒绝操作）两个路径。
///
/// 返回 true 表示错误由管理员权限缺失导致，调用者应：
/// - 仅更新本地簿记，延迟驱动级清理到下次 UAC 提权操作
/// - 或触发 UAC 提权路径
pub fn contains_elevation_indicator(text: &str) -> bool {
    let lower = text.to_lowercase();
    lower.contains("740")
        || lower.contains("提升")              // zh-CN
        || lower.contains("elevation")
        || lower.contains("elevated")
        || lower.contains("access is denied")
        || lower.contains("access denied")
        || lower.contains("privilege")
        || lower.contains("requires elevation")
        || lower.contains("administrator")
        // 多语言系统错误消息覆盖
        || lower.contains("管理者")            // ja: 管理者として実行
        || lower.contains("관리자")            // ko: 관리자 권한
        || lower.contains("verweigert")        // de: Zugriff verweigert
        || lower.contains("refusé")            // fr: Accès refusé
        || lower.contains("elevación")         // es: elevación requerida
        || lower.contains("necessária")        // pt: elevação necessária
        || lower.contains("elevata") // it: autorizzazione elevata
}

/// 虚拟端点后端的统一接口。
///
/// 每个实现负责自己的平台资源生命周期。Windows com0com 可以拥有真正的两端配对，
/// Unix PTY 则由 TauTerm 持有 master、仅暴露 slave；这些差异都不进入上层 session 模型。
///
/// # 线程安全
///
/// 所有可变方法接收 `&mut self` —— 调用者负责将实现包装在
/// `Mutex<Box<dyn VirtualPortBackend>>` 中以实现线程安全访问。
///
/// # 实现示例
///
/// ```ignore
/// // com0com (Windows)
/// impl VirtualPortBackend for VirtualPortManager { ... }
///
/// // native POSIX PTY (Linux/macOS)
/// struct PtyBackend { ... }
/// impl VirtualPortBackend for PtyBackend { ... }
/// ```
/// Send supertrait 是必需的：AppState 通过 Tauri State 在线程间共享。
pub trait VirtualPortBackend: Send {
    /// 检查后端所需资源是否存在。
    ///
    /// com0com 需要随包驱动文件；原生 PTY 后端不依赖外部二进制。
    fn are_files_present(&self) -> bool;

    /// 检测后端驱动/内核能力是否可用。
    fn detect_driver(&self) -> bool;

    /// 安装/初始化后端（普通权限路径）。
    fn install_driver(&mut self) -> Result<(), String>;

    /// 通过管理员提权安装后端驱动（UAC / sudo）。
    ///
    /// 当 `install_driver()` 因权限不足失败时调用。
    /// 返回 `Ok(())` 表示提权安装成功。
    fn install_driver_elevated(&mut self) -> Result<(), String>;

    /// 创建 `count` 个虚拟端点（普通权限路径）。
    fn create_endpoints(
        &mut self,
        config: &VirtualPortConfig,
    ) -> Result<Vec<VirtualEndpoint>, String>;

    /// 通过管理员提权创建虚拟端点；仅需要提权的后端实际使用此路径。
    fn create_endpoints_elevated(
        &mut self,
        config: &VirtualPortConfig,
    ) -> Result<Vec<VirtualEndpoint>, String>;

    /// 销毁一个虚拟端点（含优雅降级策略）。
    fn destroy_endpoint(&mut self, endpoint: &VirtualEndpoint) -> Result<(), String>;

    /// 退出时清理所有活跃端点。
    fn cleanup_all(&mut self);

    /// 启动时清理上次异常退出遗留的后端资源。
    fn cleanup_orphans(&mut self) -> u32;

    /// 通过提权批量清理残留后端资源。
    fn cleanup_endpoints_elevated(&mut self) -> Result<u32, String>;

    /// 返回后端记录的待清理资源数量。
    fn pending_orphan_count(&self) -> u32;
}
