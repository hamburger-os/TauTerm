//! VirtualPortBackend trait — 虚拟串口后端抽象接口
//!
//! 定义虚拟串口后端（如 com0com、socat、tty0tty）需要实现的操作。
//! 当前实现：com0com（通过 `VirtualPortManager`）。
//! 未来可扩展：`SocatBackend`（Linux/macOS）、`Tty0ttyBackend` 等。

use serde::{Deserialize, Serialize};

/// 虚拟串口端口对。
///
/// 表示一对已创建且保持连接的虚拟 COM 端口。
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PortPair {
    pub port_a: String,
    pub port_b: String,
    pub bus_number: u32,
}

/// 用于创建虚拟端口对的配置。
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
        || lower.contains("elevata")           // it: autorizzazione elevata
}

/// 虚拟串口后端的统一接口。
///
/// 每个实现代表一种虚拟串口方案（驱动 / 用户态工具 / 内核模块），
/// 负责管理虚拟 COM 端口对的生命周期。
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
/// // socat (Linux/macOS) — 未来扩展
/// struct SocatBackend { ... }
/// impl VirtualPortBackend for SocatBackend { ... }
/// ```
/// Send supertrait 是必需的：AppState 通过 Tauri State 在线程间共享。
pub trait VirtualPortBackend: Send {
    /// 检查后端所需的文件/二进制是否存在。
    ///
    /// 对于 com0com：检查 setupc.exe、com0com.sys 等 7 个文件。
    /// 对于 socat：检查 `socat` 是否在 PATH 中。
    fn are_files_present(&self) -> bool;

    /// 检测后端驱动/守护进程是否已安装并运行。
    fn detect_driver(&self) -> bool;

    /// 安装/初始化后端（普通权限路径）。
    fn install_driver(&mut self) -> Result<(), String>;

    /// 通过管理员提权安装后端驱动（UAC / sudo）。
    ///
    /// 当 `install_driver()` 因权限不足失败时调用。
    /// 返回 `Ok(())` 表示提权安装成功。
    fn install_driver_elevated(&mut self) -> Result<(), String>;

    /// 创建 `count` 个虚拟串口端口对（普通权限路径）。
    fn create_pairs(&mut self, config: &VirtualPortConfig) -> Result<Vec<PortPair>, String>;

    /// 通过管理员提权创建端口对（UAC / sudo），一并清理残留。
    fn create_pairs_elevated(&mut self, config: &VirtualPortConfig) -> Result<Vec<PortPair>, String>;

    /// 销毁一个虚拟端口对（含优雅降级策略）。
    fn destroy_pair(&mut self, pair: &PortPair) -> Result<(), String>;

    /// 退出时清理所有活跃端口对。
    fn cleanup_all(&mut self);

    /// 启动时清理上次异常退出遗留的端口对。
    fn cleanup_orphans(&mut self) -> u32;

    /// 通过提权批量清理残留端口对。
    fn cleanup_pairs_elevated(&mut self) -> Result<u32, String>;

    /// 返回持久化文件中记录的待清理 bus 数量。
    fn pending_orphan_count(&self) -> u32;
}
