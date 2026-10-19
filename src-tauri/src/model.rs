//! 数据类型。字段与设计文档 7.14 的 JSON 一致（camelCase）。

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Source {
    Ai,
    Manual,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ItemStatus {
    Recognizing,
    Done,
    Pending,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Item {
    pub id: String,
    pub created_at: String,
    pub text: String,
    #[serde(default)]
    pub note: String,
    pub source: Source,
    #[serde(default)]
    pub edited: bool,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub confidence: Option<f32>,
    #[serde(default)]
    pub shots: Vec<String>,
    pub status: ItemStatus,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Summary {
    pub text: String,
    pub generated_at: String,
    #[serde(default)]
    pub edited_at: Option<String>,
    #[serde(default)]
    pub previous_text: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Day {
    pub items: Vec<Item>,
    pub summary: Option<Summary>,
    /// 任何条目增删改（含标签）的时间，用来判断总结是否过期
    pub items_changed_at: Option<String>,
    pub reminded_at: Option<String>,
    pub mute_reminder: bool,
    pub missed_banner_dismissed: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Data {
    pub schema_version: u32,
    /// 键为 YYYY-MM-DD，按日期排序
    pub days: BTreeMap<String, Day>,
}

impl Default for Data {
    fn default() -> Self {
        Data { schema_version: SCHEMA_VERSION, days: BTreeMap::new() }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ModelConfig {
    #[serde(rename = "baseURL")]
    pub base_url: String,
    pub api_key: String,
    pub model: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TagOrigin {
    User,
    Ai,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TagDef {
    pub name: String,
    pub created_by: TagOrigin,
    #[serde(default)]
    pub created_at: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PromptOverrides {
    pub vision: Option<String>,
    pub summary: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct UpdateState {
    pub last_check_at: Option<String>,
    pub snoozed_version: Option<String>,
    pub snoozed_until: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub schema_version: u32,
    pub onboarding_done: bool,
    pub user_name: String,
    pub title_suffix: String,
    pub provider: String,
    pub vision: ModelConfig,
    pub summary: ModelConfig,
    /// 汇总模型是否单独配置；false 时与识图共用接口地址和 Key
    pub separate_summary: bool,
    #[serde(rename = "sendImageToAI")]
    pub send_image_to_ai: bool,
    pub tags: Vec<TagDef>,
    #[serde(rename = "allowAINewTags")]
    pub allow_ai_new_tags: bool,
    pub banned_words: Vec<String>,
    pub style_examples: String,
    pub prompt_overrides: PromptOverrides,
    pub hotkey: String,
    pub remind_time: String,
    pub weekday_only: bool,
    pub auto_start: bool,
    pub shots_retention_days: u32,
    pub update: UpdateState,
    pub close_to_tray_tip_shown: bool,
}

pub const DEFAULT_BANNED_WORDS: [&str; 7] =
    ["完成", "今天", "共计", "顺利", "进一步", "持续", "有效推进"];

impl Default for Settings {
    fn default() -> Self {
        Settings {
            schema_version: SCHEMA_VERSION,
            onboarding_done: false,
            user_name: String::new(),
            title_suffix: "工作梳理".into(),
            provider: "qwen".into(),
            vision: ModelConfig::default(),
            summary: ModelConfig::default(),
            separate_summary: false,
            send_image_to_ai: true,
            tags: Vec::new(),
            allow_ai_new_tags: true,
            banned_words: DEFAULT_BANNED_WORDS.iter().map(|s| s.to_string()).collect(),
            style_examples: String::new(),
            prompt_overrides: PromptOverrides::default(),
            hotkey: "Ctrl+Alt+S".into(),
            remind_time: "18:00".into(),
            weekday_only: true,
            auto_start: true,
            shots_retention_days: 90,
            update: UpdateState::default(),
            close_to_tray_tip_shown: false,
        }
    }
}

impl Settings {
    /// 汇总实际使用的模型配置
    pub fn summary_config(&self) -> ModelConfig {
        if self.separate_summary {
            self.summary.clone()
        } else {
            ModelConfig {
                base_url: self.vision.base_url.clone(),
                api_key: self.vision.api_key.clone(),
                model: if self.summary.model.is_empty() {
                    self.vision.model.clone()
                } else {
                    self.summary.model.clone()
                },
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_defaults_match_spec() {
        let s = Settings::default();
        assert_eq!(s.hotkey, "Ctrl+Alt+S");
        assert_eq!(s.remind_time, "18:00");
        assert!(s.weekday_only);
        assert!(s.auto_start);
        assert_eq!(s.shots_retention_days, 90);
        assert_eq!(
            s.banned_words,
            vec!["完成", "今天", "共计", "顺利", "进一步", "持续", "有效推进"]
        );
        assert_eq!(s.title_suffix, "工作梳理");
        assert!(s.allow_ai_new_tags);
        assert!(s.send_image_to_ai);
        assert!(!s.onboarding_done);
        assert_eq!(s.schema_version, 1);
    }

    #[test]
    fn item_roundtrip_json_camel_case() {
        let item = Item {
            id: "20261015-183012-a1b2".into(),
            created_at: "2026-10-15T18:30:12+08:00".into(),
            text: "项目乙报销，计算。".into(),
            note: String::new(),
            source: Source::Ai,
            edited: false,
            tags: vec!["项目乙".into()],
            confidence: Some(0.9),
            shots: vec!["shots/20261015-183012-a1b2.png".into()],
            status: ItemStatus::Done,
        };
        let json = serde_json::to_string(&item).unwrap();
        assert!(json.contains("\"createdAt\""));
        assert!(json.contains("\"status\":\"done\""));
        assert!(json.contains("\"source\":\"ai\""));
        let back: Item = serde_json::from_str(&json).unwrap();
        assert_eq!(back, item);
    }

    #[test]
    fn settings_tolerates_missing_fields() {
        // 旧版本或手改过的 settings.json 缺字段时，用默认值补齐
        let s: Settings = serde_json::from_str(r#"{"userName":"某某"}"#).unwrap();
        assert_eq!(s.user_name, "某某");
        assert_eq!(s.hotkey, "Ctrl+Alt+S");
    }
}
