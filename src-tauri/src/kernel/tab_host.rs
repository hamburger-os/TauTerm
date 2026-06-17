//! 标签页宿主模块
//!
//! 管理所有标签页的生命周期——创建、关闭、激活、重排序。
//! 标签页与会话关联，但不包含任何协议实现。

use std::collections::HashMap;
use std::sync::RwLock;

/// 标签页唯一标识符
pub type TabId = String;

/// 插件标识符
pub type PluginId = String;

/// 标签页状态
#[derive(Debug, Clone, PartialEq)]
pub enum TabState {
    Disconnected,
    Connecting,
    Connected,
    Transferring,
    Error,
}

/// 标签页信息
#[derive(Debug, Clone)]
pub struct TabInfo {
    pub id: TabId,
    pub plugin_id: PluginId,
    pub name: String,
    pub state: TabState,
}

/// 标签页宿主
///
/// 管理标签页 CRUD，不包含协议逻辑。
/// 会话创建由 Plugin Host 委托给具体插件的 `ProtocolAdapter`。
pub struct TabHost {
    /// 标签页存储
    tabs: RwLock<HashMap<TabId, TabInfo>>,
    /// 活跃标签页 ID
    active_id: RwLock<Option<TabId>>,
    /// 标签页顺序
    tab_order: RwLock<Vec<TabId>>,
    /// 最大并发标签页数
    max_tabs: usize,
}

impl TabHost {
    pub fn new(max_tabs: usize) -> Self {
        Self {
            tabs: RwLock::new(HashMap::new()),
            active_id: RwLock::new(None),
            tab_order: RwLock::new(Vec::new()),
            max_tabs,
        }
    }

    /// 创建标签页
    pub fn create_tab(
        &self,
        plugin_id: PluginId,
        name: String,
    ) -> Result<TabId, TabHostError> {
        let mut tabs = self.tabs.write().map_err(|_| TabHostError::LockError)?;
        if tabs.len() >= self.max_tabs {
            return Err(TabHostError::MaxTabsReached(self.max_tabs));
        }

        let id = uuid::Uuid::new_v4().to_string();
        let tab = TabInfo {
            id: id.clone(),
            plugin_id,
            name,
            state: TabState::Connecting,
        };

        tabs.insert(id.clone(), tab);
        drop(tabs);

        {
            let mut order = self.tab_order.write().map_err(|_| TabHostError::LockError)?;
            order.push(id.clone());
        }

        {
            let mut active = self.active_id.write().map_err(|_| TabHostError::LockError)?;
            *active = Some(id.clone());
        }

        Ok(id)
    }

    /// 关闭标签页
    pub fn close_tab(&self, tab_id: &str) -> Result<(), TabHostError> {
        let mut tabs = self.tabs.write().map_err(|_| TabHostError::LockError)?;
        tabs.remove(tab_id);

        let mut order = self.tab_order.write().map_err(|_| TabHostError::LockError)?;
        order.retain(|id| id != tab_id);

        let mut active = self.active_id.write().map_err(|_| TabHostError::LockError)?;
        if active.as_deref() == Some(tab_id) {
            *active = order.first().cloned();
        }

        Ok(())
    }

    /// 激活标签页
    pub fn activate_tab(&self, tab_id: &str) -> Result<(), TabHostError> {
        let tabs = self.tabs.read().map_err(|_| TabHostError::LockError)?;
        if !tabs.contains_key(tab_id) {
            return Err(TabHostError::TabNotFound(tab_id.to_string()));
        }
        let mut active = self.active_id.write().map_err(|_| TabHostError::LockError)?;
        *active = Some(tab_id.to_string());
        Ok(())
    }

    /// 重排序标签页
    pub fn reorder_tabs(&self, new_order: Vec<TabId>) -> Result<(), TabHostError> {
        let tabs = self.tabs.read().map_err(|_| TabHostError::LockError)?;
        for id in &new_order {
            if !tabs.contains_key(id) {
                return Err(TabHostError::TabNotFound(id.clone()));
            }
        }
        let mut order = self.tab_order.write().map_err(|_| TabHostError::LockError)?;
        *order = new_order;
        Ok(())
    }

    /// 更新标签页状态
    pub fn update_tab_state(&self, tab_id: &str, state: TabState) -> Result<(), TabHostError> {
        let mut tabs = self.tabs.write().map_err(|_| TabHostError::LockError)?;
        let tab = tabs.get_mut(tab_id).ok_or_else(|| TabHostError::TabNotFound(tab_id.to_string()))?;
        tab.state = state;
        Ok(())
    }

    /// 重命名标签页
    pub fn rename_tab(&self, tab_id: &str, new_name: &str) -> Result<(), TabHostError> {
        let mut tabs = self.tabs.write().map_err(|_| TabHostError::LockError)?;
        let tab = tabs.get_mut(tab_id).ok_or_else(|| TabHostError::TabNotFound(tab_id.to_string()))?;
        tab.name = new_name.to_string();
        Ok(())
    }

    /// 获取活跃标签页 ID
    pub fn active_id(&self) -> Option<TabId> {
        self.active_id.read().ok()?.clone()
    }

    /// 获取所有标签页信息（按 tab_order 排列）
    pub fn tabs(&self) -> Result<Vec<TabInfo>, TabHostError> {
        let tabs = self.tabs.read().map_err(|_| TabHostError::LockError)?;
        let order = self.tab_order.read().map_err(|_| TabHostError::LockError)?;
        Ok(order.iter().filter_map(|id| tabs.get(id).cloned()).collect())
    }

    /// 获取标签页信息
    pub fn get_tab(&self, tab_id: &str) -> Option<TabInfo> {
        self.tabs.read().ok()?.get(tab_id).cloned()
    }
}

/// 标签页宿主错误
#[derive(Debug, thiserror::Error)]
pub enum TabHostError {
    #[error("标签页 {0} 不存在")]
    TabNotFound(String),
    #[error("已达到最大标签页数: {0}")]
    MaxTabsReached(usize),
    #[error("内部锁错误")]
    LockError,
}
