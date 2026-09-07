//! 内建插件注册表
//!
//! PluginHost 只保存 canonical PluginManifest，不再维护第二份 descriptor/lifecycle 元数据。
//! 协议运行时生命周期属于 SessionStore；插件 UI 生命周期属于前端 PluginRegistry。

use crate::kernel::plugin_adapter::PluginManifest;
use std::collections::HashMap;

pub struct PluginHost {
    plugins: HashMap<String, PluginManifest>,
}

impl PluginHost {
    pub fn new() -> Self {
        Self {
            plugins: HashMap::new(),
        }
    }

    pub fn register_plugin(&mut self, manifest: PluginManifest) -> Result<(), PluginHostError> {
        if self.plugins.contains_key(&manifest.id) {
            return Err(PluginHostError::AlreadyRegistered(manifest.id));
        }
        self.plugins.insert(manifest.id.clone(), manifest);
        Ok(())
    }

    pub fn plugins(&self) -> Vec<&PluginManifest> {
        self.plugins.values().collect()
    }

    pub fn get_plugin(&self, plugin_id: &str) -> Option<&PluginManifest> {
        self.plugins.get(plugin_id)
    }

    pub fn has_capability(&self, plugin_id: &str, capability: &str) -> bool {
        self.plugins
            .get(plugin_id)
            .map(|manifest| manifest.capabilities.iter().any(|item| item == capability))
            .unwrap_or(false)
    }
}

impl Default for PluginHost {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, thiserror::Error)]
pub enum PluginHostError {
    #[error("插件 '{0}' 已注册")]
    AlreadyRegistered(String),
}
