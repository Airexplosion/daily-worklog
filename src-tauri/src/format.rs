//! 标题、总结清洗、格式检查。格式靠代码兜底，不只靠提示词。

use chrono::{Datelike, NaiveDate};
use regex::Regex;
use serde::Serialize;
use std::sync::LazyLock;

static THINK_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?s)<think>.*?</think>").unwrap());
static FENCE_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"```[A-Za-z]*").unwrap());
/// 行首的项目符号 + 编号，如 "- 3) "、"1. "、"2、"
static NUM_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[\s\-•>*#·]*(\d{1,3})\s*([\.、．\)）:：])\s*").unwrap());
static BULLET_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[\s\-•>·]+").unwrap());
static TITLE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[^\d\s]{1,12}\d{1,2}\.\d{1,2}[^\d]{0,10}$").unwrap());
static DIGITS_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\d+").unwrap());
static LATIN_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[A-Za-z]{2,}").unwrap());

pub const MAX_LEN: usize = 30;
pub const MIN_LEN: usize = 8;

/// `{姓名}{月}.{日}{后半句}`，月日不补零
pub fn title(name: &str, suffix: &str, date: NaiveDate) -> String {
    format!("{}{}.{}{}", name, date.month(), date.day(), suffix)
}

/// 去掉 `<think>…</think>` 和 ``` 代码块标记
pub fn strip_think(s: &str) -> String {
    let mut out = THINK_RE.replace_all(s, "").to_string();
    // 只有开头没有结尾的思考段（被截断）整段丢弃
    if let Some(pos) = out.find("<think>") {
        out.truncate(pos);
    }
    FENCE_RE.replace_all(&out, "").to_string()
}

/// 行首的编号和编号之后的内容（不判断是不是小数）
fn num_prefix(line: &str) -> Option<(u32, &str)> {
    let caps = NUM_RE.captures(line)?;
    let n = caps.get(1)?.as_str().parse().ok()?;
    Some((n, &line[caps.get(0)?.end()..]))
}

/// 按顺序认编号：第 k 条应以 k 开头（允许跳过一个号）。
/// 这样"2.3月账单核对""3.3号楼门锁报修"里紧跟编号的数字不会被当成编号
fn numbered_lines(lines: &[&str]) -> Vec<Option<String>> {
    let mut expected = 1;
    lines
        .iter()
        .map(|l| match num_prefix(l) {
            Some((n, rest)) if n == expected || n == expected + 1 => {
                expected = n + 1;
                Some(rest.to_string())
            }
            _ => None,
        })
        .collect()
}

/// 单条文字前面的编号。"3.5万收入" 这种小数不算编号
fn strip_number(line: &str) -> Option<&str> {
    let caps = NUM_RE.captures(line)?;
    let whole = caps.get(0).unwrap();
    let rest = &line[whole.end()..];
    let sep = caps.get(2).unwrap().as_str();
    if (sep == "." || sep == "．") && rest.starts_with(|c: char| c.is_ascii_digit()) {
        return None;
    }
    Some(rest)
}

/// 删掉 `#` `*` 反引号和行首的项目符号
fn clean_line(line: &str) -> String {
    let no_marks: String = line.chars().filter(|c| !matches!(c, '#' | '*' | '`')).collect();
    BULLET_RE.replace(&no_marks, "").trim().to_string()
}

#[derive(Debug, Clone, PartialEq)]
pub struct Cleaned {
    pub lines: Vec<String>,
    /// 被删掉的多余内容（标题、前言、总结语），用于提示用户
    pub removed: Vec<String>,
}

/// 清洗模型生成的总结：只留编号条目，去 Markdown，去旧编号
pub fn clean_summary(raw: &str) -> Cleaned {
    let text = strip_think(raw);
    let lines: Vec<&str> = text.lines().map(|l| l.trim()).filter(|l| !l.is_empty()).collect();
    let numbered = numbered_lines(&lines);
    let mut out = Cleaned { lines: Vec::new(), removed: Vec::new() };
    let push = |out: &mut Cleaned, l: &str| {
        let c = clean_line(l);
        if !c.is_empty() {
            out.lines.push(c);
        }
    };
    match (numbered.iter().position(|n| n.is_some()), numbered.iter().rposition(|n| n.is_some())) {
        (Some(first), Some(last)) => {
            for (i, l) in lines.iter().enumerate() {
                if i < first || i > last {
                    out.removed.push(l.to_string());
                } else {
                    push(&mut out, numbered[i].as_deref().unwrap_or(l));
                }
            }
        }
        _ => {
            for l in &lines {
                if TITLE_RE.is_match(l) || l.contains("工作梳理") {
                    out.removed.push(l.to_string());
                } else {
                    push(&mut out, l);
                }
            }
        }
    }
    out
}

/// 标题 + `1.xxx`，换行 `\r\n`
pub fn compose(title: &str, lines: &[String]) -> String {
    let mut parts = vec![title.to_string()];
    parts.extend(lines.iter().enumerate().map(|(i, l)| format!("{}.{}", i + 1, l)));
    parts.join("\r\n")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum CheckKind {
    TooLong,
    TooShort,
    Banned,
    NewNumber,
    NewWord,
    TooMany,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Check {
    /// 第几条（从 1 开始），整体问题为 None
    pub line: Option<usize>,
    pub kind: CheckKind,
    pub msg: String,
}

/// 只提醒，不改内容
pub fn check(lines: &[String], sources: &[String], banned: &[String]) -> Vec<Check> {
    let sources_lower: Vec<String> = sources.iter().map(|s| s.to_lowercase()).collect();
    let mut out = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        let n = i + 1;
        let len = line.chars().count();
        let mut add = |kind, msg: String| out.push(Check { line: Some(n), kind, msg });
        if len > MAX_LEN {
            add(CheckKind::TooLong, format!("第 {n} 条 {len} 字，超过建议的 {MAX_LEN} 字"));
        } else if len < MIN_LEN {
            add(CheckKind::TooShort, format!("第 {n} 条只有 {len} 字，少于建议的 {MIN_LEN} 字"));
        }
        for w in banned.iter().filter(|w| !w.is_empty() && line.contains(w.as_str())) {
            add(CheckKind::Banned, format!("第 {n} 条出现了\"{w}\""));
        }
        for m in DIGITS_RE.find_iter(line) {
            if !sources.iter().any(|s| s.contains(m.as_str())) {
                add(CheckKind::NewNumber, format!("第 {n} 条出现了原始记录里没有的数字\"{}\"", m.as_str()));
            }
        }
        for m in LATIN_RE.find_iter(line) {
            let w = m.as_str().to_lowercase();
            if !sources_lower.iter().any(|s| s.contains(&w)) {
                add(CheckKind::NewWord, format!("第 {n} 条出现了原始记录里没有的英文\"{}\"", m.as_str()));
            }
        }
    }
    if !sources.is_empty() && lines.len() > sources.len() {
        out.push(Check {
            line: None,
            kind: CheckKind::TooMany,
            msg: format!("条数（{}）比原始记录（{}）还多", lines.len(), sources.len()),
        });
    }
    out
}

/// 复制前只做两件事：删 `#` `*` 反引号和行首项目符号；换行统一 `\r\n`。不重编号、不删行
pub fn for_clipboard(text: &str) -> String {
    text.split('\n')
        .map(|l| {
            let l = l.trim_end_matches('\r');
            if l.trim().is_empty() {
                String::new()
            } else {
                clean_line(l)
            }
        })
        .collect::<Vec<_>>()
        .join("\r\n")
}

/// 识图单条：一行、去编号、去 Markdown、去引号、去末尾句号
/// 单条前面的"1."。提示词要求不加编号，偶尔加了也只会是 1；"3、4月""10:30"是内容，不动
fn strip_item_number(line: &str) -> &str {
    match (num_prefix(line), strip_number(line)) {
        (Some((1, _)), Some(rest)) => rest,
        _ => line,
    }
}

pub fn clean_item(raw: &str) -> String {
    let text = strip_think(raw);
    let parts: Vec<String> = text
        .lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty())
        .map(|l| clean_line(strip_item_number(l)))
        .filter(|l| !l.is_empty())
        .collect();
    let joined = parts.join("，");
    let trimmed = joined
        .trim_matches(|c| matches!(c, '“' | '”' | '"' | '「' | '」' | '\'' | '‘' | '’'))
        .trim_end_matches(['。', '.', '；', ';'])
        .trim();
    trimmed.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn title_no_zero_pad() {
        assert_eq!(title("某某", "工作梳理", d(2026, 11, 3)), "某某11.3工作梳理");
        assert_eq!(title("某某", "工作梳理", d(2026, 10, 15)), "某某10.15工作梳理");
        assert_eq!(title("某某", "日报", d(2026, 1, 5)), "某某1.5日报");
    }

    #[test]
    fn strip_think_removes_reasoning_and_fences() {
        let raw = "<think>先想想\n再想想</think>\n```json\n{\"a\":1}\n```";
        assert_eq!(strip_think(raw).trim(), "{\"a\":1}");
    }

    #[test]
    fn clean_summary_renumbers_and_strips() {
        let raw = "某某10.15工作梳理\n\n1. **项目甲周报整理提交**\n2、项目丙ppt修改\n- 3) 项目乙报销\n以上是今天的工作。";
        let c = clean_summary(raw);
        assert_eq!(c.lines, s(&["项目甲周报整理提交", "项目丙ppt修改", "项目乙报销"]));
        assert!(c.removed.contains(&"某某10.15工作梳理".to_string()));
        assert!(c.removed.contains(&"以上是今天的工作。".to_string()));
    }

    #[test]
    fn clean_summary_keeps_trailing_period_and_inner_dash() {
        let c = clean_summary("1.项目乙报销，计算。\n2.更换项目丁海报/封面图");
        assert_eq!(c.lines, s(&["项目乙报销，计算。", "更换项目丁海报/封面图"]));
        assert!(c.removed.is_empty());
    }

    #[test]
    fn clean_summary_without_numbers_keeps_lines() {
        let c = clean_summary("某某10.15工作梳理\n项目甲周报整理提交\n* 项目丙ppt修改");
        assert_eq!(c.lines, s(&["项目甲周报整理提交", "项目丙ppt修改"]));
        assert_eq!(c.removed, s(&["某某10.15工作梳理"]));
    }

    #[test]
    fn clean_summary_middle_unnumbered_line_becomes_item() {
        let c = clean_summary("1.甲事情处理\n乙事情处理\n2.丙事情处理");
        assert_eq!(c.lines, s(&["甲事情处理", "乙事情处理", "丙事情处理"]));
    }

    #[test]
    fn clean_summary_content_starting_with_digit() {
        let c = clean_summary("1.项目甲周报整理提交\n2.3月账单核对\n3.3号楼会议室门锁报修");
        assert_eq!(c.lines, s(&["项目甲周报整理提交", "3月账单核对", "3号楼会议室门锁报修"]));
        assert!(c.removed.is_empty(), "{:?}", c.removed);
        let c2 = clean_summary("1. 3月账单核对\n2. 项目乙报销");
        assert_eq!(c2.lines, s(&["3月账单核对", "项目乙报销"]));
        let c3 = clean_summary("某某10.15工作梳理\n1.3.5万收入核对\n2.对账");
        assert_eq!(c3.lines, s(&["3.5万收入核对", "对账"]));
    }

    #[test]
    fn strip_think_single_line_fence() {
        assert_eq!(strip_think("```json {\"item\":\"对账\"} ```").trim(), "{\"item\":\"对账\"}");
    }

    #[test]
    fn compose_uses_crlf_and_no_space() {
        assert_eq!(compose("T", &s(&["a", "b"])), "T\r\n1.a\r\n2.b");
        assert_eq!(compose("T", &[]), "T");
    }

    #[test]
    fn check_flags() {
        let sources = s(&["项目甲周报整理提交", "项目乙报销，计算。"]);
        let banned = s(&["完成", "今天"]);
        let long = "共享会议室使用状况反馈，沟通物业关于3号楼会议室锁坏了的事儿。"; // 33 字
        let lines = s(&[long, "项目乙报销", "完成项目甲周报整理提交", "报销3280元核对一下吧", "PDD后台核对数据"]);
        let checks = check(&lines, &sources, &banned);
        let has = |line: usize, kind: CheckKind| checks.iter().any(|c| c.line == Some(line) && c.kind == kind);
        assert!(has(1, CheckKind::TooLong));
        assert!(has(2, CheckKind::TooShort));
        assert!(has(3, CheckKind::Banned));
        assert!(has(4, CheckKind::NewNumber));
        assert!(has(5, CheckKind::NewWord));
        // 第 1 条的 21 在来源里没有，也要报
        assert!(has(1, CheckKind::NewNumber));
        assert!(checks.iter().any(|c| c.line.is_none() && c.kind == CheckKind::TooMany));
    }

    #[test]
    fn check_clean_lines_pass() {
        let sources = s(&["项目甲周报整理提交", "沟通物业3号楼会议室锁坏了"]);
        let lines = s(&["项目甲周报整理提交", "沟通物业关于3号楼会议室锁坏了的事儿"]);
        assert!(check(&lines, &sources, &s(&["完成"])).is_empty());
    }

    #[test]
    fn for_clipboard_symbols_only() {
        assert_eq!(for_clipboard("T\n1.a\n- 多写的 **b**"), "T\r\n1.a\r\n多写的 b");
        assert_eq!(for_clipboard("T\r\n1.a\r\n\r\n2.项目丁海报/封面图"), "T\r\n1.a\r\n\r\n2.项目丁海报/封面图");
        assert_eq!(for_clipboard("# 标题\n1.`x`"), "标题\r\n1.x");
    }

    #[test]
    fn clean_item_keeps_leading_digits_that_are_content() {
        assert_eq!(clean_item("3、4月账单核对"), "3、4月账单核对");
        assert_eq!(clean_item("10:30开会对接直播排期"), "10:30开会对接直播排期");
        assert_eq!(clean_item("1、项目甲周报整理提交"), "项目甲周报整理提交");
    }

    #[test]
    fn clean_item_trailing_period() {
        assert_eq!(clean_item("1. 项目乙报销计算。"), "项目乙报销计算");
        assert_eq!(clean_item("“项目甲周报整理提交”"), "项目甲周报整理提交");
        assert_eq!(clean_item("**项目丙**ppt修改\n"), "项目丙ppt修改");
        assert_eq!(clean_item("沟通物业\n3号楼锁坏了"), "沟通物业，3号楼锁坏了");
    }
}
