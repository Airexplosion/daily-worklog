//! 服务商预设。模型名以各家官网为准，用户可以在设置里改。

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Preset {
    pub id: &'static str,
    pub name: &'static str,
    pub base_url: &'static str,
    /// 空 = 这家不能识图
    pub vision_model: &'static str,
    pub summary_model: &'static str,
    /// 申请 API Key 的官方页面
    pub help_url: &'static str,
    pub can_vision: bool,
    pub note: &'static str,
}

const PRESETS: &[Preset] = &[
    Preset {
        id: "qwen",
        name: "通义千问（阿里云百炼）",
        base_url: "https://dashscope.aliyuncs.com/compatible-mode/v1",
        vision_model: "qwen-vl-max",
        summary_model: "qwen-plus",
        help_url: "https://help.aliyun.com/zh/model-studio/get-api-key",
        can_vision: true,
        note: "推荐：能识图，新用户有免费额度",
    },
    Preset {
        id: "doubao",
        name: "豆包（火山方舟）",
        base_url: "https://ark.cn-beijing.volces.com/api/v3",
        vision_model: "doubao-seed-1-6-250615",
        summary_model: "doubao-seed-1-6-250615",
        help_url: "https://console.volcengine.com/ark/region:ark+cn-beijing/apiKey",
        can_vision: true,
        note: "模型名可能需要换成方舟控制台里开通的模型或接入点 ID",
    },
    Preset {
        id: "zhipu",
        name: "智谱（BigModel）",
        base_url: "https://open.bigmodel.cn/api/paas/v4",
        vision_model: "glm-4v-plus",
        summary_model: "glm-4-flash",
        help_url: "https://open.bigmodel.cn/usercenter/apikeys",
        can_vision: true,
        note: "",
    },
    Preset {
        id: "kimi",
        name: "Kimi（月之暗面）",
        base_url: "https://api.moonshot.cn/v1",
        vision_model: "moonshot-v1-8k-vision-preview",
        summary_model: "moonshot-v1-8k",
        help_url: "https://platform.moonshot.cn/console/api-keys",
        can_vision: true,
        note: "",
    },
    Preset {
        id: "deepseek",
        name: "DeepSeek",
        base_url: "https://api.deepseek.com",
        vision_model: "",
        summary_model: "deepseek-chat",
        help_url: "https://platform.deepseek.com/api_keys",
        can_vision: false,
        note: "这家不能看图，将只根据你补的那句话生成条目；想让 AI 看截图，可以把服务商换成能识图的，再在「高级：写总结用另一家」里填 DeepSeek",
    },
    Preset {
        id: "custom",
        name: "自定义（OpenAI 兼容接口）",
        base_url: "",
        vision_model: "",
        summary_model: "",
        help_url: "",
        can_vision: true,
        note: "填写接口地址、Key 和模型名",
    },
];

pub fn all() -> &'static [Preset] {
    PRESETS
}

pub fn find(id: &str) -> Option<&'static Preset> {
    PRESETS.iter().find(|p| p.id == id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presets_have_deepseek_no_vision() {
        let ds = find("deepseek").unwrap();
        assert!(!ds.can_vision);
        assert!(ds.vision_model.is_empty());
        assert_eq!(ds.summary_model, "deepseek-chat");
        assert!(find("qwen").unwrap().can_vision);
        assert_eq!(all()[0].id, "qwen", "通义千问排第一（默认推荐）");
        assert!(find("custom").is_some());
        assert!(all().iter().all(|p| p.id == "custom" || p.base_url.starts_with("https://")));
    }
}
