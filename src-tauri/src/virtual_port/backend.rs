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

/// Session 层只关心创建能力失败的产品语义，不应该解析 Windows/setupc 文本。
#[derive(Debug, thiserror::Error)]
pub enum VirtualPortError {
    #[error("com0com driver files missing")]
    FilesMissing,
    #[error("virtual port driver is not installed after privileged initialization")]
    DriverMissing,
    #[error("{0}")]
    Permission(String),
    #[error("{0}")]
    Backend(String),
}

impl VirtualPortError {
    pub fn from_backend(message: impl Into<String>) -> Self {
        let message = message.into();
        let lower = message.to_lowercase();
        if lower.contains("driver files missing") {
            Self::FilesMissing
        } else if lower.contains("driver not installed") {
            Self::DriverMissing
        } else if contains_elevation_indicator(&message)
            || lower.contains("cancel")
            || lower.contains("取消")
        {
            Self::Permission(message)
        } else {
            Self::Backend(message)
        }
    }

    pub fn kind(&self) -> &'static str {
        match self {
            Self::FilesMissing => "files_missing",
            Self::DriverMissing => "driver_missing",
            Self::Permission(_) => "permission",
            Self::Backend(_) => "create_failed",
        }
    }
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

/// 统一权限不足检测只保留在 Windows 后端内部边界，用来把 OS/setupc 文本归一成
/// `VirtualPortError::Permission`。Serial/UI 不直接依赖这些字符串。
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

    /// Ensure the platform virtual-port driver/runtime is ready. Permission selection belongs to
    /// the backend; callers never choose between privileged and unprivileged implementations.
    fn install_driver(&mut self) -> Result<(), String>;

    /// Create endpoints for one Session. The backend owns platform allocation, privilege,
    /// transactionality and ownership persistence.
    fn ensure_endpoints(
        &mut self,
        config: &VirtualPortConfig,
    ) -> Result<Vec<VirtualEndpoint>, VirtualPortError>;

    /// Release one endpoint owned by this backend. A direct-UAC backend may defer physical removal
    /// until the next explicit privileged action, but it must preserve ownership evidence.
    fn destroy_endpoint(&mut self, endpoint: &VirtualEndpoint) -> Result<(), String>;

    fn cleanup_all(&mut self);

    /// Explicit orphan cleanup. Implementations must only remove resources with ownership evidence.
    fn cleanup_orphans(&mut self) -> Result<u32, String>;

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

    #[test]
    fn typed_error_classification_is_owned_by_backend_boundary() {
        assert_eq!(
            VirtualPortError::from_backend("com0com driver files missing").kind(),
            "files_missing"
        );
        assert_eq!(
            VirtualPortError::from_backend("User cancelled the UAC elevation prompt").kind(),
            "permission"
        );
        assert_eq!(
            VirtualPortError::from_backend("PortName COM22 in use").kind(),
            "create_failed"
        );
    }
}
