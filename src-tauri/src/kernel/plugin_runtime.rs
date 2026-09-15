//! 插件运行时目录。
//!
//! `PluginRuntime` 是后端插件身份、canonical manifest、通用 `ProtocolAdapter` 与
//! 类型化 contribution 的唯一注册表。具体插件只在应用 composition root 注册；
//! Kernel 消费插件 API/capability，不维护协议名分支或平行 adapter 字段。

use crate::kernel::plugin_adapter::{PluginId, PluginManifest, ProtocolAdapter};
use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::sync::Arc;

struct PluginEntry {
    manifest: PluginManifest,
    adapter: Option<Arc<dyn ProtocolAdapter>>,
    contributions: HashMap<TypeId, Arc<dyn Any + Send + Sync>>,
}

pub struct PluginRuntime {
    plugins: HashMap<PluginId, PluginEntry>,
    order: Vec<PluginId>,
}

impl PluginRuntime {
    pub fn new() -> Self {
        Self {
            plugins: HashMap::new(),
            order: Vec::new(),
        }
    }

    pub fn register_manifest(
        &mut self,
        manifest: PluginManifest,
    ) -> Result<PluginId, PluginRuntimeError> {
        self.insert_entry(manifest, None, HashMap::new())
    }

    pub fn register_adapter<T>(
        &mut self,
        manifest: PluginManifest,
        adapter: T,
    ) -> Result<PluginId, PluginRuntimeError>
    where
        T: ProtocolAdapter + Any + Send + Sync + 'static,
    {
        let declared = adapter
            .plugin_id()
            .ok_or_else(|| PluginRuntimeError::MissingAdapterId(std::any::type_name::<T>()))?;
        let declared = PluginId::parse(declared).map_err(PluginRuntimeError::InvalidPluginId)?;
        if manifest.id != declared {
            return Err(PluginRuntimeError::AdapterIdMismatch {
                manifest: manifest.id,
                adapter: declared,
            });
        }

        let adapter = Arc::new(adapter);
        let protocol_adapter: Arc<dyn ProtocolAdapter> = adapter.clone();
        let erased: Arc<dyn Any + Send + Sync> = adapter;
        let mut contributions = HashMap::new();
        contributions.insert(TypeId::of::<T>(), erased);
        self.insert_entry(manifest, Some(protocol_adapter), contributions)
    }

    pub fn register_contribution<T>(
        &mut self,
        plugin_id: &PluginId,
        contribution: T,
    ) -> Result<(), PluginRuntimeError>
    where
        T: Any + Send + Sync + 'static,
    {
        let entry = self
            .plugins
            .get_mut(plugin_id)
            .ok_or_else(|| PluginRuntimeError::NotRegistered(plugin_id.clone()))?;
        let type_id = TypeId::of::<T>();
        if entry.contributions.contains_key(&type_id) {
            return Err(PluginRuntimeError::ContributionAlreadyRegistered {
                plugin_id: plugin_id.clone(),
                contribution: std::any::type_name::<T>(),
            });
        }
        entry.contributions.insert(type_id, Arc::new(contribution));
        Ok(())
    }

    pub fn manifests(&self) -> Vec<&PluginManifest> {
        self.order
            .iter()
            .filter_map(|plugin_id| self.plugins.get(plugin_id).map(|entry| &entry.manifest))
            .collect()
    }

    pub fn manifest(&self, plugin_id: &PluginId) -> Option<&PluginManifest> {
        self.plugins.get(plugin_id).map(|entry| &entry.manifest)
    }

    pub fn manifest_by_str(&self, plugin_id: &str) -> Option<&PluginManifest> {
        self.plugins.get(plugin_id).map(|entry| &entry.manifest)
    }

    pub fn adapter(&self, plugin_id: &PluginId) -> Option<Arc<dyn ProtocolAdapter>> {
        self.plugins
            .get(plugin_id)
            .and_then(|entry| entry.adapter.clone())
    }

    pub fn adapter_by_str(&self, plugin_id: &str) -> Option<Arc<dyn ProtocolAdapter>> {
        self.plugins
            .get(plugin_id)
            .and_then(|entry| entry.adapter.clone())
    }

    pub fn contribution<T>(&self, plugin_id: &PluginId) -> Option<Arc<T>>
    where
        T: Any + Send + Sync + 'static,
    {
        self.plugins
            .get(plugin_id)?
            .contributions
            .get(&TypeId::of::<T>())?
            .clone()
            .downcast::<T>()
            .ok()
    }

    pub fn contribution_by_str<T>(&self, plugin_id: &str) -> Option<Arc<T>>
    where
        T: Any + Send + Sync + 'static,
    {
        self.plugins
            .get(plugin_id)?
            .contributions
            .get(&TypeId::of::<T>())?
            .clone()
            .downcast::<T>()
            .ok()
    }

    pub fn has_capability(&self, plugin_id: &PluginId, capability: &str) -> bool {
        self.manifest(plugin_id)
            .is_some_and(|manifest| manifest.capabilities.iter().any(|item| item == capability))
    }

    fn insert_entry(
        &mut self,
        manifest: PluginManifest,
        adapter: Option<Arc<dyn ProtocolAdapter>>,
        contributions: HashMap<TypeId, Arc<dyn Any + Send + Sync>>,
    ) -> Result<PluginId, PluginRuntimeError> {
        let plugin_id = manifest.id.clone();
        if self.plugins.contains_key(&plugin_id) {
            return Err(PluginRuntimeError::AlreadyRegistered(plugin_id));
        }
        self.order.push(plugin_id.clone());
        self.plugins.insert(
            plugin_id.clone(),
            PluginEntry {
                manifest,
                adapter,
                contributions,
            },
        );
        Ok(plugin_id)
    }
}

impl Default for PluginRuntime {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, thiserror::Error)]
pub enum PluginRuntimeError {
    #[error("插件 '{0}' 已注册")]
    AlreadyRegistered(PluginId),
    #[error("插件 '{0}' 未注册")]
    NotRegistered(PluginId),
    #[error("Adapter {0} 未声明 plugin_id")]
    MissingAdapterId(&'static str),
    #[error("无效插件 ID: {0}")]
    InvalidPluginId(String),
    #[error("插件 manifest ID '{manifest}' 与 Adapter ID '{adapter}' 不一致")]
    AdapterIdMismatch {
        manifest: PluginId,
        adapter: PluginId,
    },
    #[error("插件 '{plugin_id}' 已注册 contribution {contribution}")]
    ContributionAlreadyRegistered {
        plugin_id: PluginId,
        contribution: &'static str,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kernel::plugin_adapter::ProtocolConnection;
    use crate::session::SessionError;

    struct DummyAdapter;

    #[async_trait::async_trait]
    impl ProtocolAdapter for DummyAdapter {
        fn plugin_id(&self) -> Option<&'static str> {
            Some("dummy")
        }

        async fn connect(
            &self,
            _endpoint: &str,
            _params: &serde_json::Value,
        ) -> Result<ProtocolConnection, SessionError> {
            Err(SessionError::CapabilityDenied {
                capability: "dummy".into(),
            })
        }
    }

    fn manifest(id: &str) -> PluginManifest {
        PluginManifest {
            id: PluginId::parse(id).unwrap(),
            name: id.to_string(),
            version: "1".into(),
            category: "test".into(),
            description: String::new(),
            icon: "terminal".into(),
            content_type: "terminal".into(),
            send_bar: false,
            capabilities: vec!["session".into()],
            transfer_protocols: Vec::new(),
        }
    }

    #[test]
    fn adapter_manifest_and_typed_contribution_share_one_entry() {
        let mut runtime = PluginRuntime::new();
        let plugin_id = runtime
            .register_adapter(manifest("dummy"), DummyAdapter)
            .unwrap();

        assert_eq!(runtime.manifests().len(), 1);
        assert!(runtime.adapter(&plugin_id).is_some());
        assert!(runtime.contribution::<DummyAdapter>(&plugin_id).is_some());
        assert!(runtime.has_capability(&plugin_id, "session"));
    }

    #[test]
    fn manifest_only_plugin_does_not_fake_a_protocol_adapter() {
        let mut runtime = PluginRuntime::new();
        let plugin_id = runtime.register_manifest(manifest("custom")).unwrap();
        assert!(runtime.adapter(&plugin_id).is_none());
    }

    #[test]
    fn duplicate_plugin_and_duplicate_contribution_fail_closed() {
        let mut runtime = PluginRuntime::new();
        let plugin_id = runtime
            .register_adapter(manifest("dummy"), DummyAdapter)
            .unwrap();
        assert!(matches!(
            runtime.register_manifest(manifest("dummy")),
            Err(PluginRuntimeError::AlreadyRegistered(_))
        ));

        runtime.register_contribution(&plugin_id, 7_u32).unwrap();
        assert!(matches!(
            runtime.register_contribution(&plugin_id, 8_u32),
            Err(PluginRuntimeError::ContributionAlreadyRegistered { .. })
        ));
    }

    #[test]
    fn adapter_identity_must_match_manifest_identity() {
        let mut runtime = PluginRuntime::new();
        assert!(matches!(
            runtime.register_adapter(manifest("other"), DummyAdapter),
            Err(PluginRuntimeError::AdapterIdMismatch { .. })
        ));
    }
}
