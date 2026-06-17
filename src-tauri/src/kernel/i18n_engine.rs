//! 国际化引擎
//!
//! 命名空间隔离翻译、插件翻译资源注册、运行时语言切换。
//! 注意：此模块提供后端 i18n 基础设施。实际 UI 翻译由前端 i18next 处理。

use std::collections::HashMap;
use std::sync::RwLock;

/// 语言代码（如 "zh-CN", "en-US"）
pub type Locale = String;

/// 翻译键
pub type TranslationKey = String;

/// 翻译资源：键 → 翻译文本
pub type TranslationMap = HashMap<TranslationKey, String>;

/// 国际化引擎
pub struct I18nEngine {
    /// 当前语言
    current_locale: RwLock<Locale>,
    /// 命名空间 → (语言 → 翻译映射)
    /// 全局命名空间使用空字符串 ""
    resources: RwLock<HashMap<String, HashMap<Locale, TranslationMap>>>,
}

impl I18nEngine {
    pub fn new() -> Self {
        Self {
            current_locale: RwLock::new("zh-CN".into()),
            resources: RwLock::new(HashMap::new()),
        }
    }

    /// 注册插件的翻译资源
    ///
    /// `namespace` 应使用插件 ID（如 "serial", "ssh"）以避免冲突。
    pub fn register_locales(
        &self,
        namespace: &str,
        locale: &str,
        translations: TranslationMap,
    ) -> Result<(), I18nError> {
        let mut resources = self.resources.write().map_err(|_| I18nError::LockError)?;
        let ns_map = resources.entry(namespace.to_string()).or_default();
        ns_map.insert(locale.to_string(), translations);
        Ok(())
    }

    /// 翻译键
    ///
    /// `key` 格式: `"namespace:translation_key"`（带命名空间）或 `"translation_key"`（全局）。
    pub fn t(&self, key: &str) -> String {
        let (namespace, translation_key) = match key.split_once(':') {
            Some((ns, k)) => (ns, k),
            None => ("", key),
        };

        let locale = self.current_locale.read().map(|l| l.clone()).unwrap_or_default();

        if let Ok(resources) = self.resources.read() {
            if let Some(ns_map) = resources.get(namespace) {
                if let Some(trans_map) = ns_map.get(&locale) {
                    if let Some(text) = trans_map.get(translation_key) {
                        return text.clone();
                    }
                }
                // 回退到 en-US
                if locale != "en-US" {
                    if let Some(trans_map) = ns_map.get("en-US") {
                        if let Some(text) = trans_map.get(translation_key) {
                            return text.clone();
                        }
                    }
                }
            }
        }

        // 回退：返回键名本身
        translation_key.to_string()
    }

    /// 设置当前语言
    pub fn set_locale(&self, locale: &str) {
        if let Ok(mut l) = self.current_locale.write() {
            *l = locale.to_string();
        }
    }

    /// 获取当前语言
    pub fn current_locale(&self) -> String {
        self.current_locale.read().map(|l| l.clone()).unwrap_or_default()
    }
}

impl Default for I18nEngine {
    fn default() -> Self { Self::new() }
}

/// 国际化引擎错误
#[derive(Debug, thiserror::Error)]
pub enum I18nError {
    #[error("内部锁错误")]
    LockError,
}
