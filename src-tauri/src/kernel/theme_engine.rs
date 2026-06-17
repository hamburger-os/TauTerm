//! 主题引擎
//!
//! CSS 自定义属性生成、运行时主题切换、插件自定义 token 注入。

use std::collections::HashMap;
use std::sync::RwLock;

/// 主题名称
pub type ThemeName = String;

/// CSS 自定义属性集合（属性名 → 值）
pub type TokenSet = HashMap<String, String>;

/// 主题引擎
pub struct ThemeEngine {
    /// 主题名 → Token 集合
    themes: RwLock<HashMap<ThemeName, TokenSet>>,
    /// 插件贡献的 token（plugin_id → TokenSet）
    plugin_tokens: RwLock<HashMap<String, TokenSet>>,
    /// 当前活跃主题名
    active_theme: RwLock<ThemeName>,
}

impl ThemeEngine {
    pub fn new() -> Self {
        let mut themes = HashMap::new();

        // 内建主题: Neon Dark
        let mut neon_dark = TokenSet::new();
        neon_dark.insert("--bg-primary".into(), "#0a0a1a".into());
        neon_dark.insert("--bg-secondary".into(), "#12122a".into());
        neon_dark.insert("--text-primary".into(), "#e0e0ff".into());
        neon_dark.insert("--accent-color".into(), "#00ffff".into());
        neon_dark.insert("--border-color".into(), "rgba(0, 255, 255, 0.3)".into());
        themes.insert("neon-dark".into(), neon_dark);

        // 内建主题: Ocean
        let mut ocean = TokenSet::new();
        ocean.insert("--bg-primary".into(), "#0a1929".into());
        ocean.insert("--bg-secondary".into(), "#132f4c".into());
        ocean.insert("--text-primary".into(), "#b3d9ff".into());
        ocean.insert("--accent-color".into(), "#3399ff".into());
        ocean.insert("--border-color".into(), "rgba(51, 153, 255, 0.3)".into());
        themes.insert("ocean".into(), ocean);

        // 内建主题: Sunset
        let mut sunset = TokenSet::new();
        sunset.insert("--bg-primary".into(), "#1a0a0a".into());
        sunset.insert("--bg-secondary".into(), "#2a1212".into());
        sunset.insert("--text-primary".into(), "#ffe0cc".into());
        sunset.insert("--accent-color".into(), "#ff6600".into());
        sunset.insert("--border-color".into(), "rgba(255, 102, 0, 0.3)".into());
        themes.insert("sunset".into(), sunset);

        Self {
            themes: RwLock::new(themes),
            plugin_tokens: RwLock::new(HashMap::new()),
            active_theme: RwLock::new("neon-dark".into()),
        }
    }

    /// 获取当前主题的所有 token（合并插件贡献）
    pub fn active_tokens(&self) -> HashMap<String, String> {
        let mut tokens = HashMap::new();

        // 基础主题
        if let Ok(themes) = self.themes.read() {
            if let Ok(active) = self.active_theme.read() {
                if let Some(base) = themes.get(&*active) {
                    tokens.extend(base.clone());
                }
            }
        }

        // 插件 token
        if let Ok(plugin_tokens) = self.plugin_tokens.read() {
            for pt in plugin_tokens.values() {
                tokens.extend(pt.clone());
            }
        }

        tokens
    }

    /// 切换到指定主题
    pub fn apply_theme(&self, name: &str) -> Result<(), ThemeError> {
        let themes = self.themes.read().map_err(|_| ThemeError::LockError)?;
        if !themes.contains_key(name) {
            return Err(ThemeError::ThemeNotFound(name.to_string()));
        }
        let mut active = self.active_theme.write().map_err(|_| ThemeError::LockError)?;
        *active = name.to_string();
        Ok(())
    }

    /// 注册插件自定义 token
    pub fn register_tokens(&self, plugin_id: &str, tokens: TokenSet) -> Result<(), ThemeError> {
        let mut plugin_tokens = self.plugin_tokens.write().map_err(|_| ThemeError::LockError)?;
        plugin_tokens.insert(plugin_id.to_string(), tokens);
        Ok(())
    }

    /// 移除插件的 token
    pub fn unregister_tokens(&self, plugin_id: &str) {
        if let Ok(mut plugin_tokens) = self.plugin_tokens.write() {
            plugin_tokens.remove(plugin_id);
        }
    }

    /// 获取当前活跃主题名
    pub fn active_name(&self) -> String {
        self.active_theme.read().map(|n| n.clone()).unwrap_or_default()
    }

    /// 获取可用主题列表
    pub fn theme_names(&self) -> Vec<String> {
        self.themes.read().map(|t| t.keys().cloned().collect()).unwrap_or_default()
    }
}

impl Default for ThemeEngine {
    fn default() -> Self { Self::new() }
}

/// 主题引擎错误
#[derive(Debug, thiserror::Error)]
pub enum ThemeError {
    #[error("主题 '{0}' 不存在")]
    ThemeNotFound(String),
    #[error("内部锁错误")]
    LockError,
}
