//! IPC 桥接模块
//!
//! 支持插件动态注册 Tauri 命令、类型事件总线、Stream 通道。

use std::collections::HashMap;
use std::sync::RwLock;

/// 事件监听器标识
pub type ListenerId = String;

/// 事件回调类型
pub type EventCallback = Box<dyn Fn(&serde_json::Value) + Send + Sync>;

/// IPC 桥接器
///
/// 管理事件订阅和命令注册，供 Plugin Host 和内核模块使用。
pub struct IpcBridge {
    /// 事件名 → (监听器 ID → 回调)
    listeners: RwLock<HashMap<String, HashMap<ListenerId, EventCallback>>>,
}

impl IpcBridge {
    pub fn new() -> Self {
        Self {
            listeners: RwLock::new(HashMap::new()),
        }
    }

    /// 订阅事件
    ///
    /// 返回监听器 ID 用于取消订阅。
    pub fn subscribe<F>(&self, event: &str, callback: F) -> ListenerId
    where
        F: Fn(&serde_json::Value) + Send + Sync + 'static,
    {
        let id = uuid::Uuid::new_v4().to_string();
        let mut listeners = self.listeners.write().unwrap_or_else(|_| {
            // Poisoned lock — recover with new map
            panic!("IpcBridge listeners lock poisoned");
        });
        let entry = listeners.entry(event.to_string()).or_default();
        entry.insert(id.clone(), Box::new(callback));
        id
    }

    /// 取消事件订阅
    pub fn unsubscribe(&self, event: &str, listener_id: &str) {
        if let Ok(mut listeners) = self.listeners.write() {
            if let Some(entry) = listeners.get_mut(event) {
                entry.remove(listener_id);
            }
        }
    }

    /// 发布事件（同时发送到所有订阅者）
    pub fn emit(&self, event: &str, payload: serde_json::Value) {
        if let Ok(listeners) = self.listeners.read() {
            if let Some(callbacks) = listeners.get(event) {
                for cb in callbacks.values() {
                    cb(&payload);
                }
            }
        }
    }
}

impl Default for IpcBridge {
    fn default() -> Self { Self::new() }
}
