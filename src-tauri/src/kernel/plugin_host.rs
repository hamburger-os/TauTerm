//! 插件宿主模块
//!
//! 管理插件的发现、加载、初始化、激活、停用和卸载全生命周期。
//! 插件注册表存储所有已注册的插件及其 ProtocolAdapter。

use std::collections::HashMap;
use crate::kernel::tab_host::PluginId;

/// 插件状态
#[derive(Debug, Clone, PartialEq)]
pub enum PluginState {
    Discovered,
    Loaded,
    Initialized,
    Ready,
    Stopped,
    Unloaded,
}

/// 插件描述符（存储在前端注册表中）
#[derive(Debug, Clone)]
pub struct PluginDescriptor {
    pub id: PluginId,
    pub name: String,
    pub version: String,
    pub category: String,
    pub content_type: String,
    pub capabilities: Vec<String>,
    pub state: PluginState,
}

/// 插件宿主
///
/// 管理插件的全生命周期。内建插件在编译时注册，未来支持动态加载。
pub struct PluginHost {
    /// 已注册的插件（plugin_id → descriptor）
    plugins: HashMap<PluginId, PluginDescriptor>,
}

impl PluginHost {
    pub fn new() -> Self {
        Self {
            plugins: HashMap::new(),
        }
    }

    /// 注册插件
    pub fn register_plugin(&mut self, descriptor: PluginDescriptor) -> Result<(), PluginHostError> {
        if self.plugins.contains_key(&descriptor.id) {
            return Err(PluginHostError::AlreadyRegistered(descriptor.id));
        }
        self.plugins.insert(descriptor.id.clone(), descriptor);
        Ok(())
    }

    /// 获取所有已注册的插件
    pub fn plugins(&self) -> Vec<&PluginDescriptor> {
        self.plugins.values().collect()
    }

    /// 获取指定插件
    pub fn get_plugin(&self, plugin_id: &str) -> Option<&PluginDescriptor> {
        self.plugins.get(plugin_id)
    }

    /// 检查插件是否声明了某个能力
    pub fn has_capability(&self, plugin_id: &str, capability: &str) -> bool {
        self.plugins.get(plugin_id)
            .map(|p| p.capabilities.contains(&capability.to_string()))
            .unwrap_or(false)
    }

    /// 注销插件
    pub fn unregister_plugin(&mut self, plugin_id: &str) -> Result<(), PluginHostError> {
        self.plugins.remove(plugin_id)
            .map(|_| ())
            .ok_or_else(|| PluginHostError::PluginNotFound(plugin_id.to_string()))
    }
}

impl Default for PluginHost {
    fn default() -> Self { Self::new() }
}

/// 插件宿主错误
#[derive(Debug, thiserror::Error)]
pub enum PluginHostError {
    #[error("插件 '{0}' 已注册")]
    AlreadyRegistered(String),
    #[error("插件 '{0}' 不存在")]
    PluginNotFound(String),
}
