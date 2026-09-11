//! VirtualPortBackend trait — 虚拟端点后端抽象接口
//!
//! 上层只依赖“创建/销毁外部虚拟端点”的能力，不感知 Windows COM 端口对
//! 或 Unix PTY 的实现差异。Windows 的 bridge 端属于 TauTerm 内部资源，
//! 通过本模块的进程级可见性注册表从普通串口发现结果中排除。

use std::collections::HashSet;
use std::sync::{LazyLock, RwLock};

use serde::{Deserialize, Serialize};

/// 一个由 TauTerm 管理并暴露给外部工具的虚拟端点。
///
/// `bridge_path` 仅供桥接层定位内部侧；`external_path` 是用户和外部程序应打开的路径；
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

/// 当前进程内由虚拟串口子系统占用、不得作为普通 Serial 端点展示的内部路径。
///
/// 这是平台资源可见性的单一注册表：Windows 直连后端和特权服务客户端在创建/销毁
/// 虚拟端点时维护它，SerialAdapter 只做只读查询。路径在 Windows 上按不区分大小写
/// 的方式规范化；Unix PTY 后端不需要注册内部 master。
static INTERNAL_ENDPOINT_PATHS: LazyLock<RwLock<HashSet<String>>> =
    LazyLock::new(|| RwLock::new(HashSet::new()));

fn normalize_endpoint_path(path: &str) -> String {
    #[cfg(target_os = "windows")]
    {
        path.to_ascii_uppercase()
    }
    #[cfg(not(target_os = "windows"))]
    {
        path.to_string()
    }
}

pub fn register_internal_endpoint_path(path: &str) {
    if path.is_empty() {
        return;
    }
    if let Ok(mut paths) = INTERNAL_ENDPOINT_PATHS.write() {
        paths.insert(normalize_endpoint_path(path));
    }
}

pub fn unregister_internal_endpoint_path(path: &str) {
    if path.is_empty() {
        return;
    }
    if let Ok(mut paths) = INTERNAL_ENDPOINT_PATHS.write() {
        paths.remove(&normalize_endpoint_path(path));
    }
}

pub fn is_internal_endpoint_path(path: &str) -> bool {
    INTERNAL_ENDPOINT_PATHS
        .read()
        .map(|paths| paths.contains(&normalize_endpoint_path(path)))
        .unwrap_or(false)
}

/// 统一权限不足检测 — 同时用于 `Err(String)`（spawn 失败）和
/// `Ok(Output)`（setupc.exe 启动成功但内核驱动拒绝操作）两个路径。
///
/// 返回 true 表示错误由管理员权限缺失导致，调用者应延迟驱动级清理到显式提权操作。
pub fn contains_elevation_indicator(text: &str) -> bool {
    let lower = text.to_lowercase();
    lower.contains("740")
        || lower.contains("提升")
        || lower.contains("elevation")
        || lower.contains("elevated")
        || lower.contains("access is denied")
        || lower.contains("access denied")
        || lower.contains("privilege")
        || lower.contains("requires elevation")
        || lower.contains("administrator")
        || lower.contains("管理者")
        || lower.contains("관리자")
        || lower.contains("verweigert")
        || lower.contains("refusé")
        || lower.contains("elevación")
        || lower.contains("necessária")
        || lower.contains("elevata")
}

/// 虚拟端点后端的统一接口。
///
/// 每个实现负责自己的平台资源生命周期。Windows com0com 拥有真正的两端配对，
/// Unix PTY 由 TauTerm 持有 master、只暴露 slave；这些差异不进入上层 session 模型。
pub trait VirtualPortBackend: Send {
    fn are_files_present(&self) -> bool;
    fn detect_driver(&self) -> bool;
    fn install_driver(&mut self) -> Result<(), String>;
    fn install_driver_elevated(&mut self) -> Result<(), String>;

    fn create_endpoints(
        &mut self,
        config: &VirtualPortConfig,
    ) -> Result<Vec<VirtualEndpoint>, String>;

    fn create_endpoints_elevated(
        &mut self,
        config: &VirtualPortConfig,
    ) -> Result<Vec<VirtualEndpoint>, String>;

    fn destroy_endpoint(&mut self, endpoint: &VirtualEndpoint) -> Result<(), String>;
    fn cleanup_all(&mut self);
    fn cleanup_orphans(&mut self) -> u32;
    fn cleanup_endpoints_elevated(&mut self) -> Result<u32, String>;
    fn pending_orphan_count(&self) -> u32;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn internal_endpoint_registry_round_trip() {
        let path = "COM197";
        unregister_internal_endpoint_path(path);
        assert!(!is_internal_endpoint_path(path));
        register_internal_endpoint_path(path);
        assert!(is_internal_endpoint_path(path));
        unregister_internal_endpoint_path(path);
        assert!(!is_internal_endpoint_path(path));
    }

    #[test]
    fn elevation_detection_covers_supported_system_messages() {
        for message in [
            "Access is denied. (os error 740)",
            "requires elevation",
            "run as administrator",
            "需要提升权限",
            "管理者として実行してください",
            "관리자 권한이 필요합니다",
            "Zugriff verweigert",
            "Accès refusé",
            "elevación requerida",
            "elevação necessária",
            "autorizzazione elevata",
        ] {
            assert!(contains_elevation_indicator(message), "{message}");
        }
    }

    #[test]
    fn elevation_detection_rejects_unrelated_cleanup_failures() {
        for message in [
            "setupc.exe execution timed out",
            "PortName COM22 in use",
            "already exists",
            "already logged",
            "",
        ] {
            assert!(!contains_elevation_indicator(message), "{message}");
        }
    }
}
