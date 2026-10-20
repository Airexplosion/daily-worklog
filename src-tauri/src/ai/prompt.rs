//! 提示词模板（原文见设计文档 7.5）。

use crate::model::Settings;

/// 识图提示词的固定结尾，由代码追加，界面上不可改
pub const VISION_TAIL: &str =
    "只输出 JSON，不要任何其他文字：\n{\"item\":\"...\",\"tags\":[\"...\"],\"confidence\":0.0}";

const VISION_TEMPLATE: &str = r#"你是{姓名}的工作记录整理助手。用户会给你一张电脑屏幕截图，以及（可能有）用户手写的一句话补充说明。

任务：输出一条符合{姓名}工作梳理格式的工作事项，并打上标签。

规则：
1. 只输出一条，8-30字，动宾结构，口语化，不要书面美化。
2. 描述"做过的一件事"，不是"打开了某个软件""看到了某个页面"。
3. 用户补充说明的优先级高于截图内容；两者冲突时以补充说明为准。
4. 严禁编造截图和补充说明里都不存在的信息（人名、数字、项目名、地点、时间）。
5. 不要出现这些词：{禁用词}。
6. 不加编号，不换行，句末不加句号。
7. 如果截图是财务报表或表格，提炼"在做什么业务动作"，不要罗列数字。
8. 如果实在判断不出在做什么，item 输出"（待补充）"，不要瞎猜。
9. 标签 1-3 个，优先从现有标签里选：{现有标签}。{新建标签规则}
{口吻范例段}"#;

const VISION_TEXT_ONLY_TEMPLATE: &str = r#"你是{姓名}的工作记录整理助手。用户会给你一句话说明。

任务：输出一条符合{姓名}工作梳理格式的工作事项，并打上标签。

规则：
1. 只输出一条，8-30字，动宾结构，口语化，不要书面美化。
2. 描述"做过的一件事"，不是"打开了某个软件""看到了某个页面"。
3. 以用户的说明为准，只做整理，不扩写。
4. 严禁编造说明里不存在的信息（人名、数字、项目名、地点、时间）。
5. 不要出现这些词：{禁用词}。
6. 不加编号，不换行，句末不加句号。
7. 如果实在判断不出在做什么，item 输出"（待补充）"，不要瞎猜。
8. 标签 1-3 个，优先从现有标签里选：{现有标签}。{新建标签规则}
{口吻范例段}"#;

const SUMMARY_TEMPLATE: &str = r#"你是{姓名}的助手，把当天记录的若干条工作事项整理成一份工作梳理。

只输出编号条目，不要输出标题：
1.xxx
2.xxx

规则：
- 每条一行，编号从 1 开始连续，编号后不加空格。
- 保留原条目的大意和口语感；表述同一件事的重复条目要合并成一条。
- 严禁新增任何原始条目中未出现的信息（人物、数字、地点、原因）。
- 条数等于合并后的实际条数，不许凑数。
- 不要出现这些词：{禁用词}。
- 不要输出解释、前言、总结、点评。
- 输出纯文本，禁止使用 Markdown 标记（# * - 等）。
{口吻范例段}"#;

pub fn default_vision_template() -> &'static str {
    VISION_TEMPLATE
}

pub fn default_summary_template() -> &'static str {
    SUMMARY_TEMPLATE
}

fn name(s: &Settings) -> &str {
    let n = s.user_name.trim();
    if n.is_empty() { "用户" } else { n }
}

fn fill(template: &str, s: &Settings) -> String {
    let banned = if s.banned_words.is_empty() { "（无）".to_string() } else { s.banned_words.join("、") };
    let tags = if s.tags.is_empty() {
        "（暂无）".to_string()
    } else {
        s.tags.iter().map(|t| t.name.as_str()).collect::<Vec<_>>().join("、")
    };
    let new_tag_rule = if s.allow_ai_new_tags {
        "现有标签都不合适时，可以新起一个 2-6 字的短标签。"
    } else {
        "只能从现有标签里选；都不合适就用\"其他\"。"
    };
    let examples = s.style_examples.trim();
    let style = if examples.is_empty() {
        String::new()
    } else {
        format!("以下是{}以前写过的总结，学习其口吻和用词，不要照抄内容：\n{}", name(s), examples)
    };
    template
        .replace("{姓名}", name(s))
        .replace("{禁用词}", &banned)
        .replace("{现有标签}", &tags)
        .replace("{新建标签规则}", new_tag_rule)
        .replace("{口吻范例段}", &style)
        .trim_end()
        .to_string()
}

fn override_of(o: &Option<String>) -> Option<&str> {
    o.as_deref().filter(|t| !t.trim().is_empty())
}

/// 有图时由代码追加：图上的标注是用户自己加的（用户改过完整提示词也照样追加）
pub const ANNOTATION_NOTE: &str =
    "图上的框、圈、箭头、手写文字、马赛克可能是用户自己加的标注：标注指的地方是重点，不要描述标注本身，也不要猜马赛克下面的内容。";

/// 多张图时由代码追加：同一件事，按顺序看，只写一条
pub fn multi_image_note(n: usize, merged: bool) -> String {
    let mut s = format!("这次给你的是 {n} 张截图，是同一件事，按顺序看，合起来只写一条。");
    if merged {
        s.push_str("有的图是几张截图按顺序拼在一起的（从左到右或从上到下）。");
    }
    s
}

/// 识图用的系统提示词：模板（或用户改过的），再按张数追加说明，最后是固定结尾。images 是实际发出去的张数
pub fn vision_prompt(s: &Settings, images: usize, merged: bool) -> String {
    let template = override_of(&s.prompt_overrides.vision).unwrap_or(if images > 0 {
        VISION_TEMPLATE
    } else {
        VISION_TEXT_ONLY_TEMPLATE
    });
    let mut body = fill(template, s);
    if images >= 2 && override_of(&s.prompt_overrides.vision).is_none() {
        // 默认模板第一句按张数说；用户自己改过的提示词不动
        body = body.replacen("一张电脑屏幕截图", "几张电脑屏幕截图（是同一件事）", 1);
    }
    let mut parts = vec![body];
    if images >= 2 {
        parts.push(multi_image_note(images, merged));
    } else if merged {
        parts.push("这张图是几张截图按顺序拼在一起的（从左到右或从上到下），是同一件事，合起来只写一条。".to_string());
    }
    if images >= 1 {
        parts.push(ANNOTATION_NOTE.to_string());
    }
    parts.push(VISION_TAIL.to_string());
    parts.join("\n\n")
}

pub fn summary_prompt(s: &Settings) -> String {
    fill(override_of(&s.prompt_overrides.summary).unwrap_or(SUMMARY_TEMPLATE), s)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Settings, TagDef, TagOrigin};

    fn settings() -> Settings {
        let mut s = Settings::default();
        s.user_name = "某某".into();
        s
    }

    #[test]
    fn vision_prompt_fills_vars() {
        let s = settings();
        let p = vision_prompt(&s, 1, false);
        assert!(p.contains("你是某某的工作记录整理助手"));
        assert!(p.contains("（暂无）"));
        assert!(p.contains("完成、今天、共计、顺利、进一步、持续、有效推进"));
        assert!(p.contains("可以新起一个 2-6 字的短标签"));
        assert!(p.contains("截图是财务报表"));
        assert!(p.ends_with(VISION_TAIL));
        let body = &p[..p.len() - VISION_TAIL.len()];
        assert!(!body.contains('{'), "未替换的变量: {body}");
    }

    #[test]
    fn vision_prompt_disallow_new_tags() {
        let mut s = settings();
        s.allow_ai_new_tags = false;
        s.tags = vec![TagDef { name: "项目乙".into(), created_by: TagOrigin::User, created_at: String::new(), pending: false }];
        let p = vision_prompt(&s, 1, false);
        assert!(p.contains("都不合适就用\"其他\""));
        assert!(p.contains("优先从现有标签里选：项目乙"));
    }

    #[test]
    fn vision_prompt_text_only() {
        let p = vision_prompt(&settings(), 0, false);
        assert!(!p.contains("截图是财务报表"));
        assert!(p.contains("用户会给你一句话说明"));
        assert!(p.ends_with(VISION_TAIL));
    }

    #[test]
    fn style_examples_block() {
        let mut s = settings();
        s.style_examples = "某某10.14工作梳理\n1.对账".into();
        let p = vision_prompt(&s, 1, false);
        assert!(p.contains("以下是某某以前写过的总结，学习其口吻和用词，不要照抄内容：\n某某10.14工作梳理\n1.对账"));
        let p2 = summary_prompt(&s);
        assert!(p2.contains("学习其口吻和用词"));
    }

    #[test]
    fn override_still_gets_tail() {
        let mut s = settings();
        s.prompt_overrides.vision = Some("自定义：{姓名}".into());
        let p = vision_prompt(&s, 1, false);
        assert!(p.starts_with("自定义：某某"));
        assert!(p.ends_with(VISION_TAIL));
    }

    #[test]
    fn summary_prompt_no_title_instruction() {
        let p = summary_prompt(&settings());
        assert!(p.contains("你是某某的助手"));
        assert!(p.contains("只输出编号条目，不要输出标题"));
        assert!(!p.contains("{"));
    }

    #[test]
    fn vision_prompt_by_image_count() {
        let s = settings();
        let none = vision_prompt(&s, 0, false);
        assert!(!none.contains(ANNOTATION_NOTE) && !none.contains("张截图"), "{none}");
        let one = vision_prompt(&s, 1, false);
        assert!(one.contains(ANNOTATION_NOTE) && !one.contains("这次给你的是"), "{one}");
        let three = vision_prompt(&s, 3, false);
        assert!(three.contains("这次给你的是 3 张截图") && three.contains(ANNOTATION_NOTE), "{three}");
        assert!(three.ends_with(VISION_TAIL));
        assert!(!three.contains("拼在一起"));
        let merged = vision_prompt(&s, 3, true);
        assert!(merged.contains("拼在一起") && merged.contains("从左到右") && merged.contains("从上到下"), "{merged}");
    }

    #[test]
    fn vision_prompt_override_still_gets_notes() {
        let mut s = settings();
        s.prompt_overrides.vision = Some("自定义识图提示词".into());
        let p = vision_prompt(&s, 2, false);
        assert!(p.starts_with("自定义识图提示词"));
        assert!(p.contains("这次给你的是 2 张截图") && p.contains(ANNOTATION_NOTE) && p.ends_with(VISION_TAIL), "{p}");
    }

    #[test]
    fn default_template_first_sentence_follows_image_count() {
        let s = settings();
        assert!(vision_prompt(&s, 1, false).contains("一张电脑屏幕截图"));
        let three = vision_prompt(&s, 3, false);
        assert!(three.contains("几张电脑屏幕截图") && !three.contains("一张电脑屏幕截图"), "{three}");
    }
}
