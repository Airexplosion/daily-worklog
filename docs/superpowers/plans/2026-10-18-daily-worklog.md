# 每日工作梳理 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 一个 Tauri 2 的 Windows 托盘软件：热键冻结截图 → 补一句 → 后台 AI 生成带标签的条目 → 定时提醒 → 一键生成格式固定的工作梳理。

**Architecture:** Rust 后端负责全部逻辑（截屏、存储、AI、格式清洗、提醒、更新检查），纯逻辑模块都有 `cargo test`；前端是无打包工具的原生 HTML/CSS/JS（`// @ts-check`），多个 Tauri 窗口：主窗口、每块屏一个框选窗口、补充框、右下角提示窗、首次引导窗口。前后端通过 Tauri 命令和事件通信。

**Tech Stack:** Tauri 2, Rust stable (MSVC), xcap, image, reqwest (native-tls, stream), tokio, serde/serde_json, chrono, tauri-plugin-{global-shortcut, autostart, single-instance, notification, clipboard-manager, opener, log}, vanilla JS, NSIS, GitHub Actions + tauri-action.

**Spec:** `docs/superpowers/specs/2026-10-18-daily-worklog-design.md`（以下称 Spec）

## Global Constraints

- 只支持 Windows 10/11 x64；NSIS `installMode: currentUser`，`webviewInstallMode: embedBootstrapper`，安装界面简体中文。
- 前端不引框架、不用打包工具；JS 文件首行 `// @ts-check`。
- 数据目录固定 `%APPDATA%\DailyWorkLog\`（data.json / settings.json / shots\ / backups\ / logs\）。
- 标题 = `{userName}{月}.{日}{titleSuffix}`，月日不补零；编号 `N.` 后无空格；剪贴板换行 `\r\n`；输出不含 `#` `*`，行首不含 `-` `•` `>`。
- 默认禁用词：完成、今天、共计、顺利、进一步、持续、有效推进。
- 默认热键 `Ctrl+Alt+S`；默认提醒 `18:00`；默认只在工作日提醒；默认开机自启；截图默认保留 90 天。
- AI：OpenAI 兼容 `POST {baseURL}/chat/completions`；识图超时 20 秒、重试 1 次；图片长边 ≤1600、JPEG 质量 85；断网不自动重试。
- 日志里不许出现 API Key 和图片内容。
- 界面颜色、圆角、字体严格按 Spec 第 8 节的 token。
- 文案凡 Spec 用 `代码格式` 或引号写死的，原样使用。

## Review Focus

1. **模型输出脏数据**：识图返回带 ```json、`<think>`、多余解释、或标签不是数组 → 仍能得到条目，或回退为原文标黄；不丢记录（Task 3 测试）。
2. **跨零点与改系统时间**：23:59 截图、00:01 确认 → 条目归截图那天；系统时间被往回调 → 提醒不重复弹（Task 6 测试）。
3. **data.json 损坏或半写**：启动时损坏 → 从最新有效备份恢复且不覆盖好备份；`.tmp` 残留 → 忽略（Task 2 测试）。
4. **混合缩放多屏**：150% 主屏 + 100% 副屏，副屏在主屏左侧（负坐标）→ 框选窗口与裁剪都对（Task 7 spike 手测项）。
5. **标签名变体**：AI 返回"直播间 "、"直播间。"、全角"ＡＢ" → 归并为同一已有标签，不新建（Task 4 测试）。

---

## 文件结构

```
package.json                     # 仅 @tauri-apps/cli
src/                             # 前端（静态）
  index.html                     # 主窗口：今日/历史/设置
  capture.html  note.html  toast.html  onboarding.html
  styles/tokens.css base.css components.css
  js/api.js                      # invoke/listen 封装 + JSDoc 类型
  js/main.js js/today.js js/history.js js/settings.js js/summary.js
  js/capture.js js/note.js js/toast.js js/onboarding.js
  icons/*.svg                    # Fluent UI System Icons
src-tauri/
  Cargo.toml build.rs tauri.conf.json capabilities/default.json icons/
  resources/holidays/*.json      # holiday-cn 数据
  src/main.rs lib.rs             # 启动、插件、托盘、单实例
  src/model.rs                   # 数据类型 + 默认值
  src/storage.rs                 # 原子写、备份、恢复、截图清理
  src/format.rs                  # 标题、清洗、检查
  src/tags.rs                    # 标签归一、AI 标签合并、改名/合并
  src/ai/mod.rs ai/prompt.rs ai/parse.rs ai/client.rs
  src/holidays.rs                # 工作日判断
  src/reminder.rs                # 提醒判定 + 30 秒循环 + 漏生成查找
  src/capture.rs                 # 抓屏、BMP 定格、裁剪、编码
  src/windows.rs                 # 各窗口创建/定位/无激活显示
  src/recognize.rs               # 识别队列
  src/summary.rs                 # 生成总结（流式）
  src/update.rs                  # GitHub 版本检查
  src/state.rs                   # AppState
  src/commands.rs                # 所有 tauri 命令
.github/workflows/release.yml
docs/首次安装说明.md
```

---

### Task 1: 项目骨架 + 数据模型

**Files:** Create `package.json`, `src-tauri/*`（Cargo.toml、tauri.conf.json、capabilities、build.rs、main.rs、lib.rs）、`src-tauri/src/model.rs`、`src/index.html`（占位）

**Interfaces:**
- Produces: `model::{Item, ItemStatus{Recognizing,Done,Pending}, Source{Ai,Manual}, Summary, Day, Data, Settings, ModelConfig, TagDef, TagOrigin{User,Ai}, UpdateState}`，字段与 Spec 7.14 的 JSON 一致（serde `camelCase`）；`Settings::default()` 取 Global Constraints 的默认值；`Data{schema_version:1, days: BTreeMap<String, Day>}`。

- [ ] Step 1: 写测试 `settings_defaults_match_spec`（hotkey=="Ctrl+Alt+S"、remind_time=="18:00"、weekday_only、auto_start、shots_retention_days==90、banned_words 7 个按序、title_suffix=="工作梳理"、allow_ai_new_tags、send_image_to_ai）和 `item_roundtrip_json_camel_case`（序列化含 `createdAt`、`status:"done"`、`source:"ai"`）。
- [ ] Step 2: `cargo test` 失败。
- [ ] Step 3: 实现 model.rs；tauri.conf.json：productName "每日工作梳理"，identifier `com.zhaoding.dailyworklog`，NSIS 配置见 Global Constraints，主窗口 label `main` 900×640 最小 720×520 初始隐藏。
- [ ] Step 4: `cargo test` 通过；`npx tauri build --debug --no-bundle` 能编译。
- [ ] Step 5: Commit。

### Task 2: 存储

**Files:** Create `src-tauri/src/storage.rs`

**Interfaces:**
- Consumes: `model::{Data, Settings}`
- Produces: `Store::new(dir: PathBuf) -> Store`; `store.load_data() -> LoadOutcome{data: Data, recovered_from: Option<String>}`; `store.save_data(&Data) -> io::Result<()>`; `store.load_settings() -> Settings`; `store.save_settings(&Settings)`; `store.backup_on_startup() -> io::Result<()>`; `store.purge_shots(retention_days: u32, today: NaiveDate) -> usize`; `store.shots_dir() -> PathBuf`; `fn data_dir() -> PathBuf`（`%APPDATA%\DailyWorkLog`）。

- [ ] Step 1: 测试（用 tempdir）：`save_is_atomic_no_tmp_left`；`load_missing_returns_default`；`corrupt_data_restores_latest_valid_backup`（3 份备份，最新一份也坏，取第二新，`recovered_from` 为其文件名）；`backup_skipped_when_data_corrupt`；`backups_capped_at_7`；`purge_shots_removes_only_older_than_retention`（按文件名日期前缀 `YYYYMMDD-`）。
- [ ] Step 2: 运行，失败。
- [ ] Step 3: 实现：写 `*.tmp` → `sync_all` → `rename`；备份名 `data-YYYYMMDD-HHMMSS.json`。
- [ ] Step 4: 通过。
- [ ] Step 5: Commit。

### Task 3: 格式清洗、检查、AI 回复解析、提示词模板

**Files:** Create `src-tauri/src/format.rs`, `src-tauri/src/ai/parse.rs`, `src-tauri/src/ai/prompt.rs`

**Interfaces:**
- Produces:
  - `format::title(name:&str, suffix:&str, date:NaiveDate) -> String`
  - `format::strip_think(s:&str) -> String`（去 `<think>…</think>` 与 ``` 标记）
  - `format::clean_summary(raw:&str) -> Cleaned{lines: Vec<String>, removed: Vec<String>}`
  - `format::compose(title:&str, lines:&[String]) -> String`（`\r\n` 连接）
  - `format::check(lines:&[String], sources:&[String], banned:&[String]) -> Vec<Check{line: Option<usize>, kind: CheckKind, msg: String}>`，`CheckKind{TooLong,TooShort,Banned,NewNumber,TooMany}`
  - `format::clean_item(raw:&str) -> String`（单行、去编号、去 markdown、去末尾"。"）
  - `parse::parse_vision(raw:&str) -> Result<VisionReply{item:String, tags:Vec<String>, confidence:f32}, String>`（Err 携带清洗后的原文）
  - `prompt::vision_prompt(s:&Settings, with_image: bool) -> String`；`prompt::summary_prompt(s:&Settings) -> String`；`prompt::VISION_TAIL: &str`（固定 JSON 结尾）；`prompt::default_vision_template()`/`default_summary_template()`（供"恢复默认"）
- 模板与固定结尾的原文：照抄 Spec 7.5，变量 `{姓名}` `{禁用词}` `{现有标签}` `{新建标签规则}` `{口吻范例段}`。

- [ ] Step 1: 测试：
  - `title_no_zero_pad`: (张三, 工作梳理, 2026-11-03) == "张三11.3工作梳理"
  - `clean_summary_renumbers_and_strips`: 输入 "张三10.15工作梳理\n\n1. **拼多多收入确认填写**\n2、合声计划县域ppt修改\n- 3) 百应提现\n以上是今天的工作。" → lines == ["拼多多收入确认填写","合声计划县域ppt修改","百应提现"]，removed 含 "张三10.15工作梳理" 与 "以上是今天的工作。"
  - `compose_uses_crlf_and_no_space`: compose("T", ["a","b"]) == "T\r\n1.a\r\n2.b"
  - `check_flags`: 33 字行 → TooLong；6 字 → TooShort；含"完成" → Banned；"提现3280元" 而来源无 3280 → NewNumber；行数 > 来源数 → TooMany
  - `clean_item_trailing_period`: "1. 百应提现计算。" → "百应提现计算"
  - `parse_vision_variants`: 纯 JSON；```json 包裹；前面有 `<think>xx</think>`；前后有解释文字；tags 为字符串 "a、b"；缺 confidence（=0.5）；完全非 JSON → Err(原文)
  - `vision_prompt_fills_vars`: 姓名=张三、标签空 → 含"张三的工作记录整理助手"与"（暂无）"；allow_new=false → 含"都不合适就用\"其他\""；`with_image=false` → 不含"截图是财务报表"
- [ ] Step 2: 失败。 - [ ] Step 3: 实现。 - [ ] Step 4: 通过。 - [ ] Step 5: Commit。

### Task 4: 标签

**Files:** Create `src-tauri/src/tags.rs`

**Interfaces:**
- Produces: `tags::normalize(s:&str) -> Option<String>`（去首尾空白与中英文标点、全角转半角比较、截断 6 字、空则 None）；`tags::apply_ai(existing:&[TagDef], returned:&[String], allow_new:bool) -> AiTags{item_tags: Vec<String>, created: Vec<String>}`（最多 3 个、去重、优先映射到已有、不允许新建时映射为"其他"、结果为空时 ["其他"]）；`tags::rename(data:&mut Data, settings:&mut Settings, from:&str, to:&str)`（to 已存在即合并）；`tags::delete(data, settings, name)`（条目无标签则补"其他"）；`tags::usage(data) -> BTreeMap<String, usize>`。
- [ ] Step 1: 测试 `normalize_variants`（"直播间 "、"直播间。"、"「直播间」" → "直播间"；"ＡＢ" 与 "AB" 视为同名）、`apply_ai_prefers_existing`、`apply_ai_disallow_maps_to_other`、`apply_ai_caps_three`、`rename_merges_into_existing_and_updates_items`、`delete_leaves_other_when_empty`。
- [ ] Step 2–5: 失败 → 实现 → 通过 → Commit。

### Task 5: 节假日

**Files:** Create `src-tauri/src/holidays.rs`, `src-tauri/resources/holidays/2025.json`, `2026.json`（来自 NateScarlet/holiday-cn，原样）

**Interfaces:**
- Produces: `holidays::Calendar::bundled() -> Calendar`；`cal.is_workday(date: NaiveDate) -> bool`；`cal.has_year(y: i32) -> bool`。规则：数据里 `isOffDay:true` → 非工作日；`isOffDay:false` → 工作日；不在数据里 → 周一至周五。
- [ ] Step 1: 测试：2026-10-01（国庆，周四）不是工作日；2026-10-10（若数据为调休上班）是工作日——测试直接读取 JSON 中第一条 `isOffDay:false` 的日期断言为工作日、第一条落在周一至周五的 `isOffDay:true` 断言为非工作日；2030-03-04（周一，无数据）是工作日且 `has_year(2030)==false`。
- [ ] Step 2–5。

### Task 6: 提醒判定与漏生成

**Files:** Create `src-tauri/src/reminder.rs`

**Interfaces:**
- Consumes: `holidays::Calendar`, `model::{Settings, Data}`
- Produces: `reminder::should_remind(now: NaiveDateTime, s:&Settings, day: Option<&Day>, cal:&Calendar) -> bool`；`reminder::find_missed(data:&Data, today: NaiveDate) -> Option<(NaiveDate, usize)>`（今天之前最近一个有已完成条目且无 summary、且未 `missedBannerDismissed` 的日子；只看最近一个有记录的日子）；`reminder::spawn_loop(app: AppHandle)`（每 30 秒；触发后写 `remindedAt` 并 emit `remind`）。
- [ ] Step 1: 测试：17:59 false；18:00 true；已生成 false；已提醒 false；muteReminder false；周末 + weekdayOnly false；周末 + !weekdayOnly true；`find_missed` 跳过今天、跳过已忽略、只返回最近一天、最近有记录日已生成则 None。
- [ ] Step 2–5。

### Task 7: 截图 spike（冻结 → 框选 → 裁剪）⚠ 先做手测

**Files:** Create `src-tauri/src/capture.rs`, `src-tauri/src/windows.rs`, `src/capture.html`, `src/js/capture.js`, `src/styles/*.css`（tokens 先落地）

**Interfaces:**
- Produces: `capture::Frozen{monitors: Vec<FrozenMonitor{index, x, y, width, height, scale, image: RgbaImage}>}`；`capture::grab_all() -> Result<Frozen>`；`capture::to_bmp(&RgbaImage) -> Vec<u8>`；`capture::crop(&Frozen, index, rect: PhysRect{x,y,w,h}) -> RgbaImage`；`capture::ai_jpeg(&RgbaImage) -> Vec<u8>`（长边≤1600，质量 85）；自定义协议 `frozen://localhost/{index}` 返回 BMP；命令 `capture_done(index:u32, x:u32, y:u32, w:u32, h:u32)`、`capture_cancel()`；`windows::show_overlays(&Frozen)`、`hide_overlays()`、`show_no_activate(&WebviewWindow)`。
- 框选页：背景 `<img src=frozen://localhost/{i}>` 铺满，遮罩 + 选区 + 尺寸（CSS 像素 × devicePixelRatio，取整）+ 顶部提示"拖动鼠标框选　Esc 取消　右键 重选"。Esc → `capture_cancel`；右键 → 清除选区；松开 → `capture_done`（宽或高 < 4 视为点击，忽略）。
- [ ] Step 1: 单元测试 `crop_exact_pixels`（合成 100×80 图，裁 (10,20,30,40)，比对像素）、`ai_jpeg_long_side_capped`（4000×1000 → 1600×400）。
- [ ] Step 2–4: 失败 → 实现 → 通过。
- [ ] Step 5: 手测：本机所有显示器，150% 缩放下框选含文字的区域，保存的 PNG 与选区一致；记录热键到画面出现的耗时（日志打点）。不通过则 `grab_all` 改用 Windows GDI BitBlt。
- [ ] Step 6: Commit。

### Task 8: 截图主流程：热键 → 补充框 → 落盘 → 后台识别 → 提示窗

**Files:** Create `src-tauri/src/state.rs`, `src-tauri/src/recognize.rs`, `src-tauri/src/ai/client.rs`, `src-tauri/src/ai/mod.rs`, `src-tauri/src/commands.rs`, `src/note.html`, `src/js/note.js`, `src/toast.html`, `src/js/toast.js`; Modify `lib.rs`

**Interfaces:**
- Consumes: Tasks 2–4, 7
- Produces: `AppState{store, data: Mutex<Data>, settings: Mutex<Settings>, frozen: Mutex<Option<Frozen>>, pending_shot: Mutex<Option<PendingShot>>, queue: mpsc::Sender<String /*item id*/>}`；`ai::client::chat(cfg:&ModelConfig, messages: Value, timeout: Duration) -> Result<String, AiError{Network,Timeout,Http(u16,String),NoKey}>`；`ai::client::chat_stream(cfg, messages, on_delta: impl FnMut(&str)) -> Result<String, AiError>`；命令 `note_submit(text: String)`、`note_skip()`、`get_note_preview() -> String(data url)`；事件 `items-changed{date}`、`toast{kind: "recognizing"|"done"|"failed", index, text, tags, note}`。
- 行为：热键 → `grab_all` → `show_overlays`；`capture_done` → 裁剪存 PNG（`shots/{id}.png`，id=`YYYYMMDD-HHMMSS-xxxx`，时间取按热键时刻）→ 隐藏框选 → 在该屏中下部显示补充框并聚焦；`note_submit/skip` → 建条目 `Recognizing` 落盘 → 隐藏补充框 → 提示窗（无激活显示，右下角工作区）→ 入队。队列 worker：无 Key 或断网 → `Pending`；否则调用识图（`sendImageToAI=false` 用纯文字提示词，无补充文字直接 `Pending`），`parse_vision` → `clean_item` → `tags::apply_ai` → `Done`（confidence<0.5 保留数值供前端标黄）；Err(原文) → 文本=原文、confidence=0。
- [ ] Step 1: 测试 `recognize::build_messages`（with_image 时 content 含 `image_url` 且 url 以 `data:image/jpeg;base64,` 开头；末尾追加 VISION_TAIL；补充文字为空时用户消息写"（无补充说明）"）。
- [ ] Step 2–4。
- [ ] Step 5: 手测：真实 Key 走通一次；断网走通一次（条目为待处理、提示窗显示 `截图已保存，有空手动写一句`）。
- [ ] Step 6: Commit。

### Task 9: 主窗口今日页 + 条目管理

**Files:** Create `src/index.html`, `src/js/main.js`, `src/js/today.js`, `src/js/api.js`, `src/styles/components.css`; Modify `commands.rs`

**Interfaces:**
- Produces 命令：`get_day(date: String) -> Day`、`today() -> String`、`add_item(date, text) -> Item`、`update_item(id, text: Option<String>, tags: Option<Vec<String>>)`、`delete_item(id)`（软删除，返回 token）、`undo_delete(token)`、`retry_item(id)`、`retry_all(date)`、`get_shot(id) -> String(data url)`、`get_tags() -> Vec<TagDef>`、`mute_today()`。
- UI：按 Spec 7.7 与界面稿 B；双击编辑、失焦保存；Toast"已删除 · 撤销"5 秒；标签点选弹层（勾选/新建）；AI 新建标签虚线"新 · xxx"及保留/改名/并入；待处理黄底 + 重试/手动写；识别中骨架动画；右下"截一张"悬浮钮；空状态；顶部日期行 `10月15日 周四` + `今日已记录 N 条 · 18:00 提醒`。
- [ ] Step 1: 测试 `delete_then_undo_restores_position`、`delete_finalizes_after_window_removes_shot`（逻辑放在 Rust，时间注入）。
- [ ] Step 2–4。 - [ ] Step 5: 手测界面。 - [ ] Step 6: Commit。

### Task 10: 生成总结 + 复制 + 过期 + 撤销重新生成

**Files:** Create `src-tauri/src/summary.rs`, `src/js/summary.js`; Modify `commands.rs`, `today.js`

**Interfaces:**
- Produces：命令 `generate_summary(date: String, on_delta: Channel<String>) -> SummaryResult{text, checks: Vec<Check>, removed: Vec<String>, skipped_pending: usize}`、`save_summary(date, text)`、`undo_regenerate(date)`、`copy_text(text)`（调用 `format::for_clipboard(text:&str) -> String`：只删 `#` `*` 反引号与行首 `-` `•` `>`，换行统一 `\r\n`，不重编号、不删行）、`summary_stale(date) -> bool`（任何条目 `createdAt`/编辑时间晚于 `generatedAt`，或条目删除后）。0 条 → 只有标题 + 检查栏"今天还没记录"。
- [ ] Step 1: 测试 `for_clipboard_symbols_only`（`"T\n1.a\n- 多写的 **b**"` → `"T\r\n1.a\r\n多写的 b"`）、`summary_input_excludes_pending`、`stale_after_item_edit`、`regenerate_keeps_previous_for_undo`、`zero_items_title_only`。
- [ ] Step 2–4。 - [ ] Step 5: 手测流式效果与复制到微信。 - [ ] Step 6: Commit。

### Task 11: 系统集成：托盘、单实例、自启、关闭到托盘、提醒、通知、热键失败

**Files:** Modify `lib.rs`, `windows.rs`, `reminder.rs`

- 托盘菜单 `截一张 / 打开主窗口 / 生成今日总结 / 退出`，双击开主窗口；`--autostart` 不显示主窗口；关闭 → 隐藏 + 首次提示"已最小化到托盘，不会退出"；单实例唤起；热键注册失败 → 主窗口顶部错误横幅"快捷键 Ctrl+Alt+S 被其他软件占用"+"去设置里换一个"；提醒触发 → 显示主窗口 + 通知（`该下班了` / `今天的工作总结可以生成了，已记录 N 条`）；启动补弹；漏生成横幅（命令 `get_missed() -> Option<{date, count}>`、`dismiss_missed(date)`）；启动时 `backup_on_startup`、`purge_shots`，恢复过数据则横幅提示。
- [ ] Step 1: 手测清单：验收 9–12。 - [ ] Step 2: Commit。

### Task 12: 历史页

**Files:** Create `src/js/history.js`; Modify `commands.rs`

- 命令 `list_days() -> Vec<{date, count, generated: bool}>`（倒序）。界面按 Spec 7.10 与界面稿 ②。复用 today 的条目组件与 summary 组件（参数化日期）。
- [ ] Step 1: 测试 `list_days_sorted_desc_with_flags`。 - [ ] Step 2–4。 - [ ] Step 5: Commit。

### Task 13: 设置页 + 首次引导 + 测试连接 + 让 AI 识别分类

**Files:** Create `src/js/settings.js`, `src/onboarding.html`, `src/js/onboarding.js`, `src-tauri/src/ai/presets.rs`; Modify `commands.rs`, `lib.rs`

**Interfaces:**
- Produces：`presets::all() -> Vec<Preset{id, name, base_url, vision_model, summary_model, help_url, can_vision}>`（Spec 7.4 表）；命令 `get_settings() -> SettingsView`（apiKey 掩码 `sk-****abcd`）、`save_settings(patch: Value)`、`reveal_api_key()`、`test_connection() -> {vision: Result<ms>, summary: Result<ms>}`（识图发 16×16 PNG）、`suggest_tags(samples: String) -> Vec<String>`、`set_hotkey(accel) -> Result<(), String>`（先注销旧的，失败回滚）、`finish_onboarding()`、`open_data_dir()`、`tag_usage()`、`rename_tag`、`delete_tag`、`get_default_prompts()`。
- 引导 4 步按 Spec 7.11 与界面稿；未完成引导时启动只开引导窗口。
- [ ] Step 1: 测试 `mask_key`（"sk-abcdef1234" → "sk-****1234"；短于 8 位全掩）、`presets_have_deepseek_no_vision`、`suggest_tags_parse`（解析模型返回的 JSON 数组或顿号分隔文本，3–8 个、归一化）。
- [ ] Step 2–4。 - [ ] Step 5: 手测验收 15、16。 - [ ] Step 6: Commit。

### Task 14: 检查更新

**Files:** Create `src-tauri/src/update.rs`; Modify `today.js`, `settings.js`

**Interfaces:**
- Produces：`update::parse_location(loc:&str) -> Option<semver::Version>`；`update::is_newer(cur, latest) -> bool`；`update::check(repo:&str) -> Result<Option<Version>>`（GET `https://github.com/{repo}/releases/latest`，`redirect::Policy::none()`，超时 8 秒）；命令 `check_update(manual: bool)`、`snooze_update(version)`；事件 `update-available{version, url}`。仓库常量 `UPDATE_REPO` 由 `build.rs` 从环境变量 `DWL_UPDATE_REPO` 注入，缺省 `"OWNER/REPO"`（此时不检查）；发布人名字 `DWL_PUBLISHER` 同理（缺省"发布人"）。
- [ ] Step 1: 测试 `parse_location_variants`（`https://github.com/a/b/releases/tag/v1.2.0` → 1.2.0；`v1.2` → None；无 tag → None）、`snooze_3_days`。
- [ ] Step 2–4。 - [ ] Step 5: Commit。

### Task 15: 打包与发布

**Files:** Create `.github/workflows/release.yml`, `docs/首次安装说明.md`, `src-tauri/icons/*`（含 16/20/24/32/48/256 的 icon.ico）, `README.md`

- release.yml：`on: push: tags: ['v*']`，`windows-latest`，setup node + rust，校验 tag 与 `tauri.conf.json` 版本一致，`tauri-apps/tauri-action` 生成 NSIS 并创建 Release（env 注入 `DWL_UPDATE_REPO=${{ github.repository }}`）。
- [ ] Step 1: 本地 `npx tauri build` 生成安装包，确认 ≤15MB（验收 25）。
- [ ] Step 2: 用标准用户账户安装一次（若无条件则记录为未验证）。
- [ ] Step 3: Commit。
