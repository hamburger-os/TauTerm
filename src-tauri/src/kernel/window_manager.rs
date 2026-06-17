//! 窗口管理器
//!
//! 窗口创建/关闭、布局持久化、分屏状态管理。
//! 基础骨架——多窗口和分屏在后续版本完善。

use serde::{Deserialize, Serialize};

/// 分屏方向
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SplitDirection {
    Horizontal,
    Vertical,
}

/// 分屏信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SplitPane {
    pub direction: SplitDirection,
    pub sizes: Vec<f64>, // 比例数组
    pub children: Vec<PaneContent>,
}

/// 分屏内容
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PaneContent {
    Tab(String),          // 包含的标签页 ID
    Split(Box<SplitPane>), // 递归分屏
}

/// 窗口布局
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WindowLayout {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub maximized: bool,
    pub root_pane: SplitPane,
}

/// 窗口管理器
///
/// 管理窗口布局持久化和分屏状态。
pub struct WindowManager {
    /// 当前布局
    current_layout: std::sync::RwLock<Option<WindowLayout>>,
}

impl WindowManager {
    pub fn new() -> Self {
        Self {
            current_layout: std::sync::RwLock::new(None),
        }
    }

    /// 保存当前窗口布局
    pub fn save_layout(&self, layout: &WindowLayout) -> Result<(), WindowManagerError> {
        let mut current = self.current_layout.write().map_err(|_| WindowManagerError::LockError)?;
        *current = Some(layout.clone());
        Ok(())
    }

    /// 获取已保存的布局
    pub fn load_layout(&self) -> Option<WindowLayout> {
        self.current_layout.read().ok()?.clone()
    }

    /// 创建分屏
    ///
    /// 在当前活跃分屏中创建新窗格。
    pub fn split_pane(
        &self,
        tab_id: &str,
        direction: SplitDirection,
    ) -> Result<(), WindowManagerError> {
        // 基础骨架实现——完整分屏逻辑在后续版本完善
        let mut layout = self.current_layout.write().map_err(|_| WindowManagerError::LockError)?;

        if let Some(ref mut l) = *layout {
            let new_content = PaneContent::Tab(tab_id.to_string());

            let old_root = std::mem::replace(
                &mut l.root_pane,
                SplitPane {
                    direction: SplitDirection::Horizontal,
                    sizes: vec![0.5, 0.5],
                    children: vec![PaneContent::Tab(tab_id.to_string()), new_content],
                },
            );

            l.root_pane = SplitPane {
                direction,
                sizes: vec![0.5, 0.5],
                children: vec![PaneContent::Split(Box::new(old_root)), PaneContent::Tab(tab_id.to_string())],
            };
        }

        Ok(())
    }
}

impl Default for WindowManager {
    fn default() -> Self { Self::new() }
}

/// 窗口管理器错误
#[derive(Debug, thiserror::Error)]
pub enum WindowManagerError {
    #[error("内部锁错误")]
    LockError,
}
