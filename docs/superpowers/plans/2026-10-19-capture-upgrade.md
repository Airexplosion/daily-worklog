# 截图升级（标注、复制、一条多图）Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 框选完先出标注工具栏（矩形、椭圆、箭头、画笔、文字、马赛克），回车复制、Tab 记录；一条记录最多放 9 张图（连着截，或拖进、粘贴、选文件），AI 按顺序一起看、只写一条。

**Architecture:**
- **后端拆成三块**：
  - 截图会话：`start_capture(origin)` → `finish(action)`；
  - "没记完的这一条"：`Draft`，存成 `draft.json`；
  - 识图：多张图一次请求，超出上限就拼图。
- **两种状态分开管**：一次截图（`capturing` + `session`）和草稿（`draft`）可以同时存在。有草稿时开始的截图，主按钮是"加到这条"。
- **框选页面**改成 ES 模块：`js/capture/` 下放纯逻辑模块（几何、按键、图形），用 `node --test` 测；画标注、合成带标注的图在页面画布上按物理像素完成，再把 RGBA 原始字节交给后端。没有标注时仍由后端从定格原图裁剪，最快也最准。

**Tech Stack:**
- Tauri 2 + Rust：xcap、image（新增 webp 解码）、reqwest；
- 插件：tauri-plugin-clipboard-manager（写图、读图），**新增** tauri-plugin-dialog（另存为、选文件）；
- 前端：原生 JS（`// @ts-check`，ES 模块）；
- 测试：Node 22 内置 `node --test`（纯 JS 模块），CDP 端到端测试（沿用 `cdp.mjs`、`mock_ai.py`）。

**Spec:** `docs/superpowers/specs/2026-10-19-capture-upgrade-design.md`（下称 Spec）。它基于 `docs/superpowers/specs/2026-10-18-daily-worklog-design.md`（下称主文档）。

## Global Constraints

**平台与框架**
- 只支持 Win10/11 x64。前端不用框架、不用打包工具，JS 首行 `// @ts-check`。

**数据**
- 数据格式向后兼容：老 `data.json`、`settings.json` 不迁移就能读；`Item.shots` 仍是相对数据目录的路径数组（`shots/{id}.png`）。
- 只复制、只另存的截图**不在数据目录写任何文件**，也**不发给 AI**（Spec 2 反目标）。
- 一条最多 9 张图（`MAX_SHOTS = 9`）。导入的图转 PNG 存进 `shots/`，长边超过 8000 的缩到 8000 以内。

**AI 请求**
- 每张图长边 ≤1600、JPEG 质量 85，沿用 `capture::ai_jpeg`。
- 识图超时：1 张 20 秒，每多 1 张加 10 秒，最多 60 秒。网络错误、429、5xx 重试 1 次（沿用）。

**按键**

| 按键 | 作用 |
|---|---|
| Enter / 双击选框 | 主按钮 |
| Tab | 记录（有草稿时是"加到这条"） |
| Ctrl+C | 复制 |
| Ctrl+S | 保存 |
| Ctrl+Z | 撤销 |
| Esc | 取消（有草稿时回到补一句小窗） |
| 右键 | 重选（已画的标注一起清掉） |

输入法选字时（`isComposing` 或 `keyCode === 229`）不触发任何快捷键。

**主按钮**
- 有草稿 → 加到这条；
- 主窗口"截一张"、引导"试截一张" → 记录；
- 热键、托盘 → 设置 `enterAction`（`copy` 默认 / `record`）。

**文案**
- 界面文案、提示词、文档里不出现任何具体举例（需求方："所有举例都不要留"）。
- Spec 里用引号或代码格式写死的文案原样使用。
- 界面颜色、圆角、字体用 `src/styles/tokens.css` 的 token（主文档第 8 节）。

**测试**
- 不在用户桌面上模拟系统鼠标键盘：端到端测试只用 CDP 往网页里发事件，数据目录用 `DWL_DATA_DIR` 指向的测试目录。
- 剪贴板检查会覆盖用户当前的剪贴板内容，这一点在交接时说明。
- 日志里不出现 API Key 和图片内容。

## Review Focus

1. **混合缩放多屏**：标注坐标、合成图、"沿用上次的框"在 150% 屏和 100% 屏之间不能错位。
   - "沿用上次的框"在屏幕几何变了以后不能乱放。
   - 测试：Task 2 `last_rect_same_monitor_only`、Task 5 `toPhys_scale_1_5`。
2. **定格画面还没加载完就导出**：用户手快，框完马上标注并回车，此时 `#bg` 可能还没解码完。
   - 要求：标注工具在画面就绪前不可用；导出前 `await bg.decode()`。
   - 无标注走后端裁剪，不受影响。
   - 测试：Task 6 端到端 `fast-annotate`。
3. **草稿文件损坏、图被删**：启动恢复时不能崩。
   - `draft.json` 是半截 JSON → 当作没有草稿；草稿里的图文件不在了 → 去掉；一张不剩 → 丢弃草稿。
   - 测试：Task 1 `corrupt_draft_loads_as_none`、`recover_drops_missing_files`。
4. **奇怪的导入文件**：超大图（12000 像素）、webp、改了扩展名的文本文件、0 字节文件。
   - 要求：大图缩到 8000；读不出来的计数、提示，其余照加。
   - 测试：Task 7 `decode_*`、`fit_within_caps_long_side`。
5. **输入法**：文字工具里用中文输入法选字时按回车，只能上屏，不能复制截图，也不能结束这段文字。补一句小窗同理（沿用）。
   - 测试：Task 5 `keys_ignore_ime_composition`、`keys_text_editing_swallow_enter`。

---

## 文件结构

```
package.json                       # 加 "type": "module" 和 "test:js" 脚本
tests/js/geometry.test.mjs         # 新：选框几何、工具栏位置
tests/js/keys.test.mjs             # 新：按键 → 动作
tests/js/shapes.test.mjs           # 新：箭头、马赛克、样式记忆
src/capture.html                   # 改：选框拖动点、工具栏、颜色粗细行、预览画布、文字输入框
src/js/capture.js                  # 删：拆进 js/capture/
src/js/capture/geometry.js         # 新：纯函数
src/js/capture/keys.js             # 新：纯函数
src/js/capture/shapes.js           # 新：纯函数 + 画图（ctx）
src/js/capture/overlay.js          # 新：框选页面的事件与状态
src/note.html  src/js/note.js      # 改：缩略图一排、再截一张、加图片、粘贴、拖入、不记了
src/bar.html   src/js/bar.js       # 新："已截 N 张"小条
src/toast.html src/js/toast.js     # 改：张数、"已复制"
src/js/today.js                    # 改：从图片添加、拖入遮罩、Ctrl+V
src/js/itemlist.js src/js/ui.js    # 改：叠放缩略图 + 张数、大图翻页
src/js/api.js                      # 改：captureStart、addImagePaths 等
src/js/settings.js src/js/onboarding.js src/js/guide.js   # 改："框选后按回车"、文案
src/styles/main.css                # 改：叠放缩略图、拖入遮罩、大图翻页
docs/首次安装说明.md  README.md    # 改：文案
src-tauri/Cargo.toml               # 加 tauri-plugin-dialog；image 加 webp
src-tauri/src/draft.rs             # 新：草稿
src-tauri/src/capture_mode.rs      # 新：来源、动作、主按钮、上次的框、文件名
src-tauri/src/import.rs            # 新：导入图片（排序、解码、缩小、提示语）
src-tauri/src/model.rs             # 改：EnterAction、Settings.enter_action / last_save_dir
src-tauri/src/storage.rs           # 改：draft.json 读写
src-tauri/src/state.rs             # 改：PendingShot → draft / session / last_rect
src-tauri/src/flow.rs              # 改：会话、finish、草稿操作、导入
src-tauri/src/windows.rs           # 改：补一句小窗尺寸与内容、小条窗口、提示窗字段
src-tauri/src/commands.rs          # 改：新命令
src-tauri/src/recognize.rs         # 改：多图、超时、拼图重试
src-tauri/src/ai/prompt.rs         # 改：按张数追加说明、标注说明
src-tauri/src/ai/presets.rs        # 改：每家一次最多几张
src-tauri/src/capture.rs           # 改：group_sizes、stack_vertical
src-tauri/src/system.rs            # 改：托盘来源、提醒避让草稿、小条关闭
src-tauri/src/settings_cmds.rs     # 改：引导结束写默认 enterAction、试截一张的来源
src-tauri/src/lib.rs               # 改：插件、定格画面 CORS、启动恢复草稿、注册命令
```

**端到端测试**放在会话 scratchpad 的 `e2e/` 目录（下称 `$E`），沿用以下脚本：
- `cdp.mjs`、`mock_ai.py`、`run_app.sh`；
- 新增 `tcap.mjs`（按模式分场景）和 `run_cap.sh`（每个场景重启程序、换测试数据）；
- 新增夹具 `fixture-settings-v2.json`：在 `fixture-settings.json` 的基础上加 `"enterAction": "copy"`。

编译测试用的程序：`npx tauri build --debug --no-bundle`，产物在 `src-tauri/target/debug/DailyWorkLog.exe`，`run_app.sh` 用的就是它。

---

### Task 1: 草稿（没记完的这一条）的数据、存储、启动恢复

**Files:**
- Create: `src-tauri/src/draft.rs`
- Modify: `src-tauri/src/storage.rs`（`draft.json` 读写）、`src-tauri/src/lib.rs`（`mod draft;`，启动恢复）

**Interfaces:**
- Consumes: `model::{Item, ItemStatus, Source}`、`storage::write_atomic`
- Produces:
  ```rust
  pub const MAX_SHOTS: usize = 9;
  #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
  #[serde(rename_all = "camelCase")]
  pub struct Draft {
      pub id: String,            // = 第一张图的 shot_id，也是将来条目的 id
      pub date: String,          // YYYY-MM-DD，第一张图截下/加入的那天
      pub created_at: String,    // RFC3339
      pub shots: Vec<String>,    // shots/{id}.png，按顺序
      #[serde(default)] pub note: String,
      pub monitor: (i32, i32, u32, u32, f64), // 补一句小窗和小条放在哪块屏（物理像素 + 缩放）
  }
  impl Draft {
      pub fn new(id: &str, date: &str, created_at: &str, monitor: (i32, i32, u32, u32, f64)) -> Draft;
      /// 按顺序加，满 9 张为止；返回实际加了几张（多出来的由调用方删文件、出提示）
      pub fn add_shots(&mut self, rels: &[String]) -> usize;
      /// 去掉第 index 张；只剩一张或越界时不动，返回 None
      pub fn remove_shot(&mut self, index: usize) -> Option<String>;
      pub fn is_full(&self) -> bool;
      /// 记成条目：识别中、来源 AI、文字空、补充说明去首尾空白
      pub fn into_item(self) -> Item;
  }
  /// 启动恢复：去掉已经不存在的图；一张都不剩返回 None
  pub fn recover(draft: Draft, exists: impl Fn(&str) -> bool) -> Option<Item>;
  // storage.rs（Store 上）
  pub fn save_draft(&self, d: &Draft) -> io::Result<()>;   // write_atomic 到 {dir}/draft.json
  pub fn load_draft(&self) -> Option<Draft>;               // 不存在、读不出、JSON 坏 → None（坏的记 warn 日志）
  pub fn clear_draft(&self);                               // 删 draft.json，不存在不报错
  ```

- [ ] **Step 1: 写失败的测试**（`draft.rs` 的 `#[cfg(test)]`，以及 `storage.rs` 现有测试模块）
  ```rust
  #[test] fn add_shots_caps_at_nine() {
      // 先有 1 张，再加 10 张 → 返回 8，shots.len() == 9，顺序 = 原 1 张 + 新的前 8 张
  }
  #[test] fn remove_shot_keeps_last_one() {
      // 只有 1 张：remove_shot(0) == None，仍 1 张
      // [a,b,c]：remove_shot(1) == Some("b")，剩 [a,c]；remove_shot(5) == None
  }
  #[test] fn into_item_is_recognizing_with_all_shots() {
      // note "  x  " → item.note == "x"；id/created_at 来自草稿；shots 顺序不变
      // status == Recognizing，source == Ai，text == ""，tags 空
  }
  #[test] fn recover_drops_missing_files() {
      // shots [a,b,c]，exists 只认 a、c → Some(item.shots == [a,c])；都不存在 → None
  }
  #[test] fn draft_roundtrip_and_clear() {   // storage.rs，用 tempfile
      // save → load 相等；clear → load None；再 clear 不 panic
  }
  #[test] fn corrupt_draft_loads_as_none() { // storage.rs：写入 "{" → load_draft() == None
  }
  ```
- [ ] **Step 2: 跑测试确认失败**：`cd src-tauri && cargo test --lib draft`
  Expected: 编译失败，提示找不到 `draft` 模块或 `save_draft`。
- [ ] **Step 3: 实现 `draft.rs` 和 Store 的三个方法**
- [ ] **Step 4: 启动恢复**（`lib.rs` 的 `setup`）
  - 位置：在"识别中 → 待处理"那段之后、`spawn_worker` 之后。
  - 有草稿时：`draft::recover(d, |rel| store.dir().join(rel).exists())` → 有条目就放进 `data.days[date].items` 末尾，设 `items_changed_at`，`save_data`，`recognize::enqueue`，然后 `clear_draft()`。
  - 草稿一张图都不剩也要 `clear_draft()`。
  - 写一行 info 日志：`启动时把上次没记完的一条记下了（N 张图）`。
- [ ] **Step 5: 跑测试确认通过**：`cd src-tauri && cargo test --lib`
  Expected: 全部通过（原有 117 个加新增的）。
- [ ] **Step 6: 提交**
  ```bash
  git add src-tauri/src/draft.rs src-tauri/src/storage.rs src-tauri/src/lib.rs
  git commit -m "feat: 草稿（没记完的这一条）的存储与启动恢复"
  ```

### Task 2: 截图场景判断与新设置项

**Files:**
- Create: `src-tauri/src/capture_mode.rs`
- Modify: `src-tauri/src/model.rs`、`src-tauri/src/lib.rs`（`mod capture_mode;`）

**Interfaces:**
- Consumes: `capture::PhysRect`、`model::Settings`
- Produces:
  ```rust
  // model.rs
  #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
  #[serde(rename_all = "lowercase")]
  pub enum EnterAction { Copy, Record }
  // Settings 新增两个字段（Default 均为 None，serde 键名 enterAction / lastSaveDir）：
  pub enter_action: Option<EnterAction>,
  pub last_save_dir: Option<String>,

  // capture_mode.rs
  #[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
  #[serde(rename_all = "lowercase")]
  pub enum Origin { Hotkey, Tray, Main, Onboarding }
  #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
  #[serde(rename_all = "lowercase")]
  pub enum Action { Copy, Record, Save, Add }
  pub fn primary(origin: Origin, has_draft: bool, enter: Option<EnterAction>) -> Action;
  /// Tab /"记录"：有草稿时就是"加到这条"；"加到这条"在没有草稿时退回"记录"；复制、保存不变
  pub fn resolve(action: Action, has_draft: bool) -> Action;
  pub const UPGRADE_HINT: &str = "现在回车是复制，记一条请按 Tab；想改回回车记录，去设置里改";
  pub fn needs_upgrade_hint(s: &Settings) -> bool;   // onboarding_done && enter_action.is_none()
  #[derive(Debug, Clone, Copy, PartialEq)]
  pub struct LastRect { pub index: usize, pub monitor: (i32, i32, u32, u32), pub rect: PhysRect }
  pub fn last_rect_for(last: Option<&LastRect>, index: usize, monitor: (i32, i32, u32, u32)) -> Option<PhysRect>;
  pub fn save_file_name(t: chrono::NaiveDateTime) -> String; // "截图_YYYYMMDD_HHMMSS.png"
  ```

- [ ] **Step 1: 写失败的测试**
  ```rust
  #[test] fn primary_by_origin() {
      assert_eq!(primary(Origin::Hotkey, false, None), Action::Copy);
      assert_eq!(primary(Origin::Hotkey, false, Some(EnterAction::Record)), Action::Record);
      assert_eq!(primary(Origin::Tray, false, Some(EnterAction::Copy)), Action::Copy);
      assert_eq!(primary(Origin::Main, false, Some(EnterAction::Copy)), Action::Record);
      assert_eq!(primary(Origin::Onboarding, false, None), Action::Record);
      for o in [Origin::Hotkey, Origin::Tray, Origin::Main, Origin::Onboarding] {
          assert_eq!(primary(o, true, Some(EnterAction::Copy)), Action::Add);
      }
  }
  #[test] fn resolve_record_and_add() {
      assert_eq!(resolve(Action::Record, true), Action::Add);
      assert_eq!(resolve(Action::Add, false), Action::Record);
      assert_eq!(resolve(Action::Copy, true), Action::Copy);
      assert_eq!(resolve(Action::Save, true), Action::Save);
  }
  #[test] fn upgrade_hint_only_for_onboarded_without_choice() {
      // 默认设置（没走完引导）→ false；onboarding_done=true 且 None → true；Some(Copy) → false
  }
  #[test] fn last_rect_same_monitor_only() {
      // last = {index 1, monitor (-1920,0,1920,1080), rect {10,20,300,200}}
      // 同 index 同几何 → Some(rect)；index 0 → None；几何变成 (-2880,0,2880,1620) → None；last None → None
  }
  #[test] fn save_file_name_format() {
      // 2026-10-19 10:30:12 → "截图_20261019_103012.png"
  }
  #[test] fn old_settings_have_no_enter_action() { // model.rs
      // 不含 enterAction 的 settings JSON 反序列化 → enter_action == None，last_save_dir == None
  }
  #[test] fn apply_patch_enter_action() { // config.rs
      // {"enterAction":"record"} → Some(Record)；{"enterAction":"x"} → Err
  }
  ```
- [ ] **Step 2: 跑测试确认失败**：`cd src-tauri && cargo test --lib capture_mode`
  Expected: 编译失败（模块和类型不存在）。
- [ ] **Step 3: 实现**
  - `capture_mode.rs` 全是纯函数。
  - `model.rs` 加枚举和两个字段，`Default` 里都是 `None`。
  - `config::apply_patch` 不用改：枚举值不合法时 serde 会报"设置格式不对"。
- [ ] **Step 4: 跑测试确认通过**：`cd src-tauri && cargo test --lib`
  Expected: 全部通过。
- [ ] **Step 5: 提交**：`git commit -m "feat: 截图来源/动作/主按钮判断，框选后回车设置项"`

### Task 3: 截图会话后端：复制、保存、记录（单张）

**Files:**
- Modify: `src-tauri/Cargo.toml`（`tauri-plugin-dialog = "2"`）
- Modify: `src-tauri/src/state.rs`
  - 删掉 `PendingShot` / `pending_shot`；
  - 新增 `draft: Mutex<Option<Draft>>`、`session: Mutex<Option<Session>>`、`last_rect: Mutex<Option<LastRect>>`。
- Modify: `src-tauri/src/flow.rs`、`src-tauri/src/windows.rs`、`src-tauri/src/commands.rs`
- Modify: `src-tauri/src/lib.rs`（注册 dialog 插件和新命令）、`src-tauri/src/system.rs`（托盘来源）
- Modify: `src-tauri/src/settings_cmds.rs`（试截一张的来源）
- Modify: `src/js/capture.js`（最小改动，保证中途可用）、`src/js/today.js` / `src/js/api.js`（`captureStart('main')`）
- Modify: `src/toast.html` / `src/js/toast.js`（`copied`）
- Test: `$E/tcap.mjs` 的 `copy`、`upgrade-hint`、`record-one`、`pixels` 四个模式

**Interfaces:**
- Consumes: Task 1 `Draft`、`Store::{save_draft, clear_draft}`；Task 2 `Origin`、`Action`、`primary`、`resolve`、`needs_upgrade_hint`、`UPGRADE_HINT`、`LastRect`、`last_rect_for`、`save_file_name`
- Produces:
  ```rust
  // state.rs
  pub struct Session { pub origin: Origin, pub primary: Action, pub hint: Option<String> }

  // flow.rs
  pub fn start_capture(app: AppHandle, origin: Origin);
  #[derive(Clone, Serialize)] #[serde(rename_all = "camelCase")]
  pub struct CaptureInfo {
      pub url: String,
      pub primary: Action,
      pub shot_no: usize,                 // 有草稿时是第几张（草稿张数 + 1），否则 1
      pub last_rect: Option<RectView>,    // 物理像素，仅"加到这条"且同屏同几何时有
      pub hint: Option<String>,           // 升级提示，只出现一次
  }
  #[derive(Clone, Copy, Serialize)] pub struct RectView { pub x: u32, pub y: u32, pub w: u32, pub h: u32 }
  pub fn overlay_init(app: &AppHandle, index: usize) -> Option<CaptureInfo>; // capture:start 事件的内容也是它
  pub fn finish(app: &AppHandle, index: usize, img: image::RgbaImage, action: Action) -> Result<(), String>;
  pub fn cancel(app: &AppHandle);
  pub fn show_draft_note(app: &AppHandle, backdrop: bool, message: Option<String>);
  pub fn commit_draft(app: &AppHandle, note: &str) -> Result<(), String>;

  // windows.rs
  pub const NOTE_SIZE: (f64, f64) = (660.0, 300.0);
  #[derive(Clone, Serialize)] #[serde(rename_all = "camelCase")]
  pub struct NoteOpen {
      pub index: usize,          // 今天第 N 条
      pub shots: Vec<String>,    // http://shot.localhost/{文件名}
      pub note: String,
      pub max: usize,            // 9
      pub message: Option<String>,
  }
  // ToastPayload 新增 pub shots: usize；kind 新增 "copied"（text 是标题，约 1.5 秒消失）
  ```
  命令（`commands.rs`）：
  - `capture_start(origin: Origin)`：替换 `capture_from_main`。`main` 时先藏主窗口、置 `restore_main`，等 250ms 再开始。
  - `overlay_init(index) -> Option<CaptureInfo>`
  - `capture_done(index, x, y, w, h, action: Action) -> Result<(), String>`：从定格原图裁剪后调 `finish`。
  - `capture_pixels(request: tauri::ipc::Request) -> Result<(), String>`：同步命令。
    - body 是 RGBA 原始字节（`InvokeBody::Raw`），头里带 `x-index`、`x-width`、`x-height`、`x-action`。
    - 校验 `len == w*h*4`，拷出字节后开线程调 `finish`，失败时 `cancel`。
  - `note_current() -> Option<NoteOpen>`、`note_submit(text)`、`note_skip()`：后两个改调 `commit_draft`。

- [ ] **Step 1: 写端到端测试** `$E/tcap.mjs`，并写 `$E/run_cap.sh`（仿 `run_minor.sh`：每个模式重启程序；第一个参数选设置夹具）。

  先说明两个约定：
  - 下面"截一次"指在主窗口页面里执行 `invoke('capture_start', {origin})`，再 `connect('capture.html?i=0')` 连上框选页。
  - `inv(p, cmd, args)` 同 `tminor.mjs`。

  | 模式 | 设置夹具 | 步骤 | 检查 |
  |---|---|---|---|
  | `copy` | v2 | 记下 `shots/` 文件数和 `requests.jsonl` 行数 → 以 `hotkey` 截一次 → 在框选页 `overlay_init(0)` → `capture_done(0, 0, 0, 200, 100, 'copy')` | ① `info.primary === 'copy'`；② PowerShell `Add-Type -AssemblyName System.Windows.Forms; $i=[Windows.Forms.Clipboard]::GetImage(); "$($i.Width)x$($i.Height)"` 输出 `200x100`；③ `shots/` 文件数、`requests.jsonl` 行数不变；④ 提示窗页面 `#title` 含"已复制" |
  | `upgrade-hint` | 旧（`fixture-settings`） | 以 `hotkey` 截两次，每次取 `overlay_init` 后 `capture_cancel` | ① 第一次 `hint` 等于 `UPGRADE_HINT`；② 之后 `settings.json` 的 `enterAction === 'copy'`；③ 第二次 `hint === null` |
  | `record-one` | v2 | 以 `main` 截一次 → `capture_done(…,'record')` → 连 `note.html`，等 `.th` 出现 → 在 `#text` 输入"测试一句"并回车 | ① `primary === 'record'`；② 补一句小窗显示 1 张缩略图；③ `draft.json` 存在；④ 回车后 `data.json` 今天多一条，`shots.length === 1`；⑤ `draft.json` 不在了；⑥ 等识别完成后 `status === 'done'` |
  | `pixels` | v2 | 以 `main` 截一次，在框选页执行 `invoke('capture_pixels', new Uint8Array([255,0,0,255, 255,0,0,255, 255,0,0,255, 255,0,0,255]), {headers: {'x-index':'0','x-width':'2','x-height':'2','x-action':'record'}})` | `draft.json` 里那张 png 是 2×2，左上像素是 `255,0,0`（PowerShell `System.Drawing.Bitmap` 的 `GetPixel(0,0)`） |

  `record-one` 的第②点要等 Task 8 才有 `.th`。本任务先检查 `note_current()` 返回的 `shots.length === 1`。

- [ ] **Step 2: 编译并跑测试，确认失败**：`npx tauri build --debug --no-bundle && bash $E/run_cap.sh copy upgrade-hint record-one pixels`
  Expected: FAIL，命令 `capture_start` 不存在。
- [ ] **Step 3: 实现后端**
  - **`start_capture`**
    - 计算 `primary`、`hint`，存进 `session`。
    - 需要升级提示时，把 `enter_action` 设为 `Some(Copy)` 并保存设置。
    - 其余沿用原流程：抓屏、框选窗口、5 秒兜底。
    - 本任务里还没有草稿状态下的截图（Task 4 做），有草稿时先照旧直接返回。
  - **`finish`**：先 `resolve(action, has_draft)`，再按动作处理：
    - **Copy**：`app.clipboard().write_image(&tauri::image::Image::new_owned(rgba, w, h))`，收起框选窗口，`capturing = false`，弹 `copied` 提示"已复制"，`restore_main_if_needed`。
    - **Save**：先收起框选窗口、`capturing = false`；再开线程弹另存为对话框：
      - 默认目录：`last_save_dir`，没有就用 `app.path().picture_dir()`；
      - 默认文件名：`save_file_name(now)`；
      - 过滤器：`PNG`（`png`）；
      - 选了路径：补 `.png` 扩展名，写 `capture::png_bytes`，更新 `last_save_dir` 并保存设置，提示"已保存到 {文件名}"（`copied` 类）；
      - 取消：什么都不做。
      - 最后 `restore_main_if_needed`。
    - **Record**：
      - `id = capture::shot_id(taken_at)`，写 `shots/{id}.png`；
      - `Draft::new` 后 `add_shots`，`save_draft`；
      - 记 `last_rect`（屏、几何、选区）；
      - `keep_backdrop(index)`，`show_draft_note(app, true, None)`。
    - **Add**：Task 4 做。本任务里返回 `Err("没有正在记的这一条")`。
  - **`cancel`**：同原来，`session = None`。
  - **`commit_draft`**：
    - 取出草稿，`clear_draft`，收起补一句小窗和框选窗口；
    - 把这次的 `note` 写进草稿，再 `into_item()`，放进当天并保存；
    - 提示窗 `recognizing`，带 `shots` 张数；
    - `enqueue`，`restore_main_if_needed`。
  - **其他调用方**
    - 托盘"截一张"传 `Origin::Tray`，热键传 `Origin::Hotkey`，`finish_onboarding(true)` 传 `Origin::Onboarding`。
    - `on_window_event` 的 `SkipNote` 改调 `commit_draft(app, "")`。
- [ ] **Step 4: 最小前端改动**
  - `capture.js` 松手后调 `capture_done(..., action: 'record')`，保持现在的用法能用，Task 5 会整体重写。
  - 补一句小窗暂时只显示第一张缩略图（`shots[0]`），Task 8 重做。
  - `today.js` 的悬浮按钮改用 `api.captureStart('main')`。
  - `toast.js` 支持 `copied`：图标 ✓，标题 = `text`，第二行隐藏，`scheduleHide(1500)`。
  - `recognizing` 的标题在 `shots > 1` 时写成 `已记录第 N 条 · N 张图 · AI 识别中…`。
- [ ] **Step 5: 跑测试确认通过**
  Run: `cd src-tauri && cargo test --lib && cd .. && npx tauri build --debug --no-bundle && bash $E/run_cap.sh copy upgrade-hint record-one pixels`
  Expected: 单元测试全过；四个模式全部 PASS。
- [ ] **Step 6: 提交**：`git commit -m "feat: 截图会话：复制到剪贴板、另存为、记录进草稿，原始像素命令"`

### Task 4: 多张图的截图流程（后端）

**Files:**
- Modify: `src-tauri/src/flow.rs`、`src-tauri/src/windows.rs`（小条窗口）、`src-tauri/src/commands.rs`、`src-tauri/src/system.rs`、`src-tauri/src/lib.rs`
- Create: `src/bar.html`、`src/js/bar.js`（本任务只要能显示文字和按钮，样式在 Task 8）
- Test: `$E/tcap.mjs` 的 `multi`、`discard`、`crash` 三个模式

**Interfaces:**
- Consumes: Task 1、Task 2、Task 3 的全部 Produces
- Produces:
  ```rust
  // windows.rs
  pub const BAR_SIZE: (f64, f64) = (560.0, 64.0);
  #[derive(Clone, Serialize)] #[serde(rename_all = "camelCase")]
  pub struct BarView { pub count: usize, pub hotkey: String }
  pub fn show_bar(app: &AppHandle, monitor: (i32, i32, u32, u32, f64), view: BarView); // 那块屏顶部居中，不抢焦点
  pub fn hide_bar(app: &AppHandle);
  // create_aux_windows 里提前建好 "bar" 窗口：focusable(false)、置顶、透明、不在任务栏

  // flow.rs
  pub fn note_more(app: &AppHandle);                 // 收起小窗和变暗背景，显示小条
  pub fn bar_done(app: &AppHandle);                  // 收起小条，回到小窗（不变暗）
  pub fn note_text(app: &AppHandle, text: &str);     // 更新草稿的 note 并存盘
  pub fn note_blur(app: &AppHandle);                 // 不在截图时：收起变暗背景
  pub fn note_remove_shot(app: &AppHandle, index: usize) -> Result<(), String>; // 删文件、存草稿、刷新小窗
  pub fn discard_draft(app: &AppHandle);             // 删这条的图、清草稿、收起窗口
  ```
  命令：`note_more`、`note_text(text)`、`note_blur`、`note_remove_shot(index)`、`note_discard`、`bar_done`、`bar_current() -> Option<BarView>`。

- [ ] **Step 1: 写端到端测试**（`tcap.mjs`）

  `multi`（设置夹具 v2）：
  1. 以 `main` 截一次，`capture_done(0, 100, 100, 300, 200, 'record')`。
  2. `note_text('留着的字')`，检查 `draft.json` 的 `note`。
  3. `note_more`。检查：补一句小窗不可见，`bar` 可见。可见性在对应页面执行 `getCurrentWindow().isVisible()`。
  4. 以 `hotkey` 截一次，读 `overlay_init(0)`。检查：`primary === 'add'`，`shotNo === 2`，`lastRect` 是 `{x:100, y:100, w:300, h:200}`，`bar` 已收起。
  5. `capture_done(0, 100, 100, 300, 200, 'add')`。检查：草稿 2 张，补一句小窗可见，`note_current().note === '留着的字'`。
  6. 以 `hotkey` 截一次，然后 `capture_cancel`。检查：补一句小窗回来，仍 2 张。
  7. 以 `hotkey` 截一次，然后 `capture_done(…,'copy')`。检查：小窗回来，仍 2 张，`shots/` 文件数仍 2。
  8. 补一句小窗开着时以 `hotkey` 截一次。检查：小窗被收起，`overlay_init` 的 `primary === 'add'`。然后 `capture_cancel`。
  9. `note_remove_shot(0)`，检查剩 1 张、第一张的文件已删。再 `capture_start` 加一张，回到 2 张。
  10. `note_submit('留着的字')`。检查：今天多一条，`shots.length === 2` 且顺序对，`draft.json` 不在了。

  `discard`：以 `main` 截一次，记录，再 `note_discard`。检查：没有新条目，`shots/` 里没有这张，`draft.json` 不在了，`requests.jsonl` 没有新行。

  `crash`：
  1. 以 `main` 截一次，记录，`note_text('崩溃前的字')`，`note_more`。
  2. `taskkill //IM DailyWorkLog.exe //F`。
  3. 用同一个数据目录重新启动：`run_app.sh` 加环境变量 `KEEP_DATA=1`，跳过清空数据目录。
  4. 检查：今天多一条，1 张图，`note === '崩溃前的字'`，最后变成 `done`，`draft.json` 不在了。
- [ ] **Step 2: 跑测试确认失败**：`npx tauri build --debug --no-bundle && bash $E/run_cap.sh multi discard crash`
  Expected: FAIL（`note_more` 不存在）。
- [ ] **Step 3: 实现**
  - **`start_capture`**
    - 已在截图（`capturing`）→ 直接返回。
    - 有草稿 → 先收起补一句小窗、小条、变暗背景，等 150ms 再抓屏。
    - 主按钮由 `primary(origin, true, …)` 得出（`Add`），`shot_no = 张数 + 1`。
    - `overlay_init` 里按 `last_rect_for` 给 `last_rect`。
  - **`finish(Add)`**
    - 草稿满了：不加，`show_draft_note(false, Some("一条最多 9 张，多出来的没加"))`。
    - 没满：写 png，`add_shots`，`save_draft`，记 `last_rect`，`keep_backdrop(index)`，`show_draft_note(true, None)`。
  - **有草稿时，Copy / Save / cancel 结束后**一律 `show_draft_note(false, None)`，不调 `restore_main_if_needed`。
  - **`note_blur`**：`capturing` 为 false 时 `hide_overlays`。
  - **`check_reminder`**：`capturing` 或有草稿都先跳过。
  - **`close_action`**：新增 `"bar" => Hide`。
  - **托盘"退出"**：不用改。草稿留在磁盘上，下次启动由 Task 1 的恢复记下。
- [ ] **Step 4: `bar.html` / `bar.js`**
  - 内容：`已截 {count} 张 · 翻到下一屏后按 <kbd>{hotkey}</kbd> 截第 {count+1} 张`，加一个按钮"不截了，去写说明"。
  - 按钮 → `invoke('bar_done')`。
  - 监听 `bar:show` 事件；页面启动时调 `bar_current` 补一次。
- [ ] **Step 5: 跑测试确认通过**
  Run: `cd src-tauri && cargo test --lib && cd .. && npx tauri build --debug --no-bundle && bash $E/run_cap.sh multi discard crash copy record-one`
  Expected: 全部 PASS。
- [ ] **Step 6: 提交**：`git commit -m "feat: 再截一张、小条、回到补一句、去掉某张、不记了、提醒避让草稿"`

### Task 5: 框选页：选框调整、工具栏、按键（前端）

**Files:**
- Modify: `package.json`（加 `"type": "module"`，`"scripts"` 加 `"test:js": "node --test tests/js/"`）
- Create: `src/js/capture/geometry.js`、`src/js/capture/keys.js`、`src/js/capture/overlay.js`
- Modify: `src/capture.html`（改成 `<script type="module" src="js/capture/overlay.js">`）
- Delete: `src/js/capture.js`
- Test: `tests/js/geometry.test.mjs`、`tests/js/keys.test.mjs`；`$E/tcap.mjs` 的 `toolbar` 模式

**Interfaces:**
- Consumes: 命令 `overlay_init`、`overlay_ready`、`capture_done`、`capture_cancel`；事件 `capture:start` / `capture:backdrop` / `capture:reset`（`CaptureInfo` 字段见 Task 3）
- Produces（`geometry.js`，纯函数，矩形都是 `{x, y, w, h}`）：
  ```js
  export function toPhys(rect, scale)          // 两个角分别 Math.round 后相减，返回整数
  export function fromPhys(rect, scale)
  export function normalize(a, b)              // 两点 → 正宽高矩形
  export function hitHandle(rect, p, tol = 6)  // 'nw'|'n'|'ne'|'e'|'se'|'s'|'sw'|'w'|'move'|null
  export function resizeRect(rect, handle, dx, dy, view, min = 4) // 越过对边时翻转，结果限制在 view 内
  export function moveRect(rect, dx, dy, view)
  export function toolbarPos(sel, tb, view, gap = 8) // → {x, y, place: 'below'|'above'|'inside'}，x 限制在 [gap, view.w - tb.w - gap]
  ```
  `keys.js`：
  ```js
  /** @returns {'primary'|'record'|'copy'|'save'|'undo'|'cancel'|null} */
  export function keyAction(ev, ctx) // ev: {key, ctrlKey, shiftKey, isComposing, keyCode}; ctx: {hasRect, editingText}
  ```
  页面上的约定（端到端测试和 Task 6 依赖）：
  - 工具栏 `#toolbar`；
  - 动作按钮带 `data-act="save|record|copy|add|cancel|undo"`；
  - 工具按钮带 `data-tool="rect|ellipse|arrow|pen|text|mosaic"`，本任务先显示、点了没反应；
  - 提示行 `#hint`；
  - 选框 `#sel` 带 8 个拖动点 `.h[data-h]`。

- [ ] **Step 1: 写失败的单元测试**
  ```js
  // geometry.test.mjs
  test('toPhys_scale_1_5', () => assert.deepEqual(toPhys({x:10.4,y:20.2,w:100.3,h:50.1}, 1.5), {x:16,y:30,w:150,h:75}));
  test('toolbar_below_when_space', () => assert.deepEqual(
    toolbarPos({x:100,y:100,w:400,h:200}, {w:360,h:40}, {w:1920,h:1080}), {x:100,y:308,place:'below'}));
  test('toolbar_above_when_no_room_below', () => assert.deepEqual(
    toolbarPos({x:100,y:900,w:400,h:170}, {w:360,h:40}, {w:1920,h:1080}), {x:100,y:852,place:'above'}));
  test('toolbar_inside_when_full_screen', () => assert.deepEqual(
    toolbarPos({x:0,y:20,w:1920,h:1050}, {w:360,h:40}, {w:1920,h:1080}), {x:8,y:1022,place:'inside'}));
  test('toolbar_x_clamped', () => assert.equal(
    toolbarPos({x:1800,y:100,w:100,h:100}, {w:360,h:40}, {w:1920,h:1080}).x, 1552));
  test('hitHandle_corners_edges_move', ...); // 角上 → 'nw'/'se'；边中点 → 'n'；里面 → 'move'；外面 20px → null
  test('resize_flips_past_opposite_edge', ...); // {100,100,200,100} 拖 'nw' (+300,+0) → {x:300,y:100,w:100,h:100}
  test('move_clamped_to_view', ...);

  // keys.test.mjs
  const k = (key, o = {}) => ({ key, ctrlKey: false, shiftKey: false, isComposing: false, keyCode: 0, ...o });
  test('enter_is_primary_only_with_rect', () => {
    assert.equal(keyAction(k('Enter'), {hasRect: true, editingText: false}), 'primary');
    assert.equal(keyAction(k('Enter'), {hasRect: false, editingText: false}), null);
  });
  test('shortcuts', ...); // Tab→record，Ctrl+C→copy，Ctrl+S→save，Ctrl+Z→undo，Escape→cancel（没有选框也算 cancel）；没有选框时 Tab/Ctrl+C → null
  test('keys_ignore_ime_composition', ...); // isComposing 或 keyCode 229 → 一律 null
  test('keys_text_editing_swallow_enter', ...); // editingText 时 Enter/Escape/Ctrl+Z/Tab → null
  ```
- [ ] **Step 2: 跑测试确认失败**：`npm run test:js`
  Expected: FAIL，模块不存在。
- [ ] **Step 3: 实现 `geometry.js`、`keys.js`，跑 `npm run test:js` 确认通过**
- [ ] **Step 4: 写端到端测试 `toolbar` 模式**
  1. 以 `hotkey` 截一次。
  2. 在框选页用 CDP `Input.dispatchMouseEvent` 从 (200,200) 拖到 (600,500)。检查：`#toolbar` 可见；按钮从左到右依次是 `save`、`record`、`copy`，最后一个 `copy` 有 `.primary`；`#hint` 是 `Enter 复制　Tab 记录　Ctrl+S 保存　Esc 取消`。
  3. 拖右下角拖动点到 (700,560)。检查：`#size` 显示的尺寸变了。
  4. 按 Tab。检查：补一句小窗出现，草稿 1 张，图的尺寸等于 `toPhys` 后的选框。
  5. `note_more`，再以 `hotkey` 截一次。检查：选框已经在原位，主按钮是 `add`，文字含"加到这条（第 2 张）"，`#hint` 含"已截 1 张"。
  6. 按 Enter。检查：草稿 2 张。
  7. 在框选页双击选框中间，也要触发主按钮：再截一次后双击，检查草稿 3 张。
  8. 右键清掉选框，再按 Esc。检查：回到补一句小窗。
- [ ] **Step 5: 跑测试确认失败**：`npx tauri build --debug --no-bundle && bash $E/run_cap.sh toolbar`
  Expected: FAIL（还是旧页面，没有 `#toolbar`）。
- [ ] **Step 6: 重写 `capture.html` + `overlay.js`**
  - **保留原有机制**：
    - 先显示暗层，定格画面加载好后换上；
    - `overlay_init` 主动来要；
    - `capture:backdrop` 和 `capture:reset` 的处理；
    - 按 `w` 参数算缩放；
    - 用 pointer 事件、坐标限制在本屏。
  - **新增**：
    - 松手后保留选框，显示 8 个拖动点和工具栏（位置用 `toolbarPos`）。
    - 在选框里按下 → 移动；在拖动点上 → 调大小；在框外 → 重新拖一个框。只有还没有标注时才允许重新拖。
    - `CaptureInfo.lastRect` 不为空时，用 `fromPhys` 换成 CSS 像素后直接摆好选框、出工具栏。
  - **动作按钮**：
    - 有三种排法：
      - 主按钮是复制：`保存…` `记录 Tab` `复制 Enter`
      - 主按钮是记录：`保存…` `复制 Ctrl+C` `记录 Enter`
      - 主按钮是加到这条：`保存…` `复制 Ctrl+C` `加到这条（第 N 张） Enter`
    - 最后一个按钮蓝色（`.primary`），之后是 ✕。
  - **按键和鼠标**：
    - 按键：`keyAction` 的结果交给 `act(name)`。Tab 要 `preventDefault`。
    - 双击选框 = 主按钮。右键：有选框就重选，没有就取消。
  - **提示行**：
    - 框选前：`拖动鼠标框选　Esc 取消　右键 重选`
    - 框选后，按主按钮分三种：
      - 复制：`Enter 复制　Tab 记录　Ctrl+S 保存　Esc 取消`
      - 记录：`Enter 记录　Ctrl+C 复制　Ctrl+S 保存　Esc 取消`
      - 加到这条：`已截 {shotNo-1} 张 · 回车 沿用上次的框，或重新拖一个`
    - 有 `hint` 时，框选后的提示行换成 `hint`，用醒目的黄色样式（`--warn-bg`、`--warn-text`）。
  - **`act(name)`**：
    - `cancel` → `capture_cancel`；
    - `undo` → Task 6 实现，这里先空着；
    - 其余（`primary` / `record` / `copy` / `save` / `add`）→ `capture_done(index, ...toPhys(rect, scale), action)`。
      - `primary` 换成 `info.primary`；
      - `record` 原样传，后端 `resolve` 会在有草稿时变成 `add`。
    - 发出去以后 `busy = true`；失败时恢复可操作。
  - **样式**：工具栏、按钮照 `annotate-toolbar.html` 预览稿（Win11 风，`#f9f9f9` 底、圆角 7px、阴影 `--shadow-flyout`）。
- [ ] **Step 7: 跑测试确认通过**
  Run: `npm run test:js && npx tauri build --debug --no-bundle && bash $E/run_cap.sh toolbar multi copy`
  Expected: 全部 PASS。
- [ ] **Step 8: 提交**：`git commit -m "feat: 框选后可调整选框，工具栏与快捷键（复制/记录/保存/加到这条）"`

### Task 6: 标注工具

**Files:**
- Create: `src/js/capture/shapes.js`、`tests/js/shapes.test.mjs`
- Modify: `src/js/capture/overlay.js`、`src/capture.html`（预览画布 `#paint`、颜色粗细行 `#style`、文字输入框 `#textin`）
- Modify: `src-tauri/src/lib.rs`：`frozen` 协议的响应加 `Access-Control-Allow-Origin: *`。页面上 `bg.crossOrigin = 'anonymous'` 要在设置 `src` 之前，否则画布被污染，读不出像素。
- Test: `$E/tcap.mjs` 的 `annotate`、`fast-annotate` 两个模式

**Interfaces:**
- Consumes: Task 5 `geometry.js`、`keys.js`、`overlay.js` 的 `act()`；Task 3 命令 `capture_pixels`
- Produces（`shapes.js`）：
  ```js
  export const COLORS = ['#e81123', '#ffb900', '#16c60c', '#0078d4', '#1b1b1b', '#ffffff'];
  export const WIDTHS = [2, 4, 8];         // 线宽，CSS 像素（画时 × 缩放）
  export const FONT_SIZES = [14, 20, 28];  // 文字工具的三档
  export const MOSAIC_CELLS = [8, 14, 22]; // 马赛克的三档
  /** 实心三角箭头头部的三个点（物理像素） */
  export function arrowHead(x1, y1, x2, y2, width) // 头长 L = max(10, width*4)，半宽 W = max(5, width*2.5)
  /** 就地把 rect 内像素变成 cell×cell 的均色块；超出图片的部分裁掉 */
  export function pixelate(data, imgW, imgH, rect, cell)
  /** 每个工具记住自己的 {color, size}；读不到、坏了、storage 抛错都回默认（红、第 2 档） */
  export function loadStyle(storage)
  export function saveStyle(storage, style)
  /** 画一个图形；坐标是选区内的物理像素。马赛克在 base 像素上算 */
  export function drawShape(ctx, shape, scale, base)
  // shape: {tool: 'rect'|'ellipse'|'arrow'|'pen'|'text'|'mosaic', color, size, x1, y1, x2, y2, points?, text?}
  ```

- [ ] **Step 1: 写失败的单元测试**
  ```js
  test('arrow_head_horizontal', () => {
    const [tip, a, b] = arrowHead(0, 0, 100, 0, 4);  // L=16, W=10
    assert.deepEqual(tip, [100, 0]);
    assert.deepEqual([a, b].map(([x, y]) => [Math.round(x), Math.round(y)]).sort(), [[84, -10], [84, 10]]);
  });
  test('pixelate_blocks_are_uniform_average', ...); // 4×2 图，cell 2，rect 全图 → 左右两个 2×2 块各等于自己 4 个像素的平均
  test('pixelate_leaves_outside_untouched', ...);   // rect 只盖左半，右半像素不变
  test('pixelate_clamps_rect_outside_image', ...);  // rect 超出图片边界不抛错
  test('style_roundtrip_and_fallbacks', ...);       // 存了再读一致；JSON 坏 → 默认；getItem 抛错 → 默认
  ```
- [ ] **Step 2: 跑测试确认失败**：`npm run test:js`
  Expected: FAIL（`shapes.js` 不存在）。
- [ ] **Step 3: 实现 `shapes.js`，跑测试确认通过**
- [ ] **Step 4: 写端到端测试**

  `annotate`：
  1. 以 `main` 截一次，框选 (200,200) 到 (600,500)。
  2. 选椭圆工具，从 (250,250) 拖到 (350,330)。
  3. 选马赛克工具，从 (400,220) 拖到 (560,300)。
  4. 选文字工具，在 (260,420) 点一下，用 CDP `Input.insertText` 输入"标注"后按 Enter。
  5. 检查：Enter 不触发记录（草稿不存在），文字已经上画布，选框拖动点隐藏。
  6. 按 Ctrl+Z，检查文字没了（`shapes.length === 2`，页面暴露 `window.__shapes` 供测试读，只读）；再写一次文字。
  7. 按 Tab 记录。检查：
     - 存下的 png 尺寸等于选框物理尺寸；
     - 马赛克区里相隔 1 像素的两个点（同一块里）颜色相同；
     - 椭圆边上一点是红色（`R > 200 && G < 60`）。
     - 像素用 PowerShell `System.Drawing.Bitmap.GetPixel` 读，坐标按缩放换算。
  8. 检查 `localStorage` 记住了马赛克工具的档位：再截一次，马赛克工具当前档位还是刚才那档。

  `fast-annotate`：
  - 截一次后立刻（不等画面就绪）框选并画一个矩形，按 Tab。
  - 检查：记录成功，图不是全透明（左上像素 alpha = 255），也不是纯黑。
- [ ] **Step 5: 跑测试确认失败**：`npx tauri build --debug --no-bundle && bash $E/run_cap.sh annotate fast-annotate`
  Expected: FAIL。
- [ ] **Step 6: 实现**
  - **预览画布 `#paint`**
    - 盖在选框上：`width` / `height` 属性 = 物理尺寸，CSS 尺寸 = 选框 CSS 尺寸。
    - 每次变化都重画：先 `drawImage(bg, 物理 x, y, w, h, 0, 0, w, h)`，再按顺序 `drawShape`。
    - 马赛克读的是画到这一步为止的画布像素。
  - **画面就绪前**：工具按钮禁用（`#bg.ready` 之前）。
  - **工具交互**
    - 点工具按钮选中，再点一次取消选中。选中工具后，在选框里按下就画；没选工具时按下是移动选框。
    - 画了第一笔以后，选框固定，拖动点隐藏。
    - 椭圆按住 Shift 是正圆。
  - **文字工具**
    - 点一下，在点击处放 `#textin`（字号 = 档位 × 缩放 / 缩放 = 档位 CSS 像素）。
    - Enter 或失焦 → 变成 `text` 图形；Esc → 放弃这段字。
    - 输入法选字时的回车不处理。
    - 打字期间 `editingText = true`。
  - **颜色粗细行 `#style`**
    - 选了工具后出现在工具栏下方：6 个颜色点，3 档粗细。
    - 文字工具显示字号，马赛克只显示格子大小、不显示颜色。
    - 改了就 `saveStyle(localStorage, …)`。
  - **撤销**：`act('undo')` → 弹出最后一个图形并重画。
  - **右键重选**：清空图形，选框解除固定。
  - **导出**：有图形时，`await bg.decode()` → `getImageData(0, 0, w, h)` → `invoke('capture_pixels', data, {headers})`。没有图形时仍走 `capture_done`。
  - **测试接口**：`window.__shapes` 只在测试里读，用 `Object.defineProperty` 暴露一个只读 getter。
- [ ] **Step 7: 跑测试确认通过**
  Run: `npm run test:js && npx tauri build --debug --no-bundle && bash $E/run_cap.sh annotate fast-annotate toolbar copy`
  Expected: 全部 PASS。
- [ ] **Step 8: 提交**：`git commit -m "feat: 标注工具：矩形、椭圆、箭头、画笔、文字、马赛克、撤销，记住颜色粗细"`

### Task 7: 导入已有图片（后端 + "今天"页入口）

**Files:**
- Create: `src-tauri/src/import.rs`
- Modify: `src-tauri/Cargo.toml`（`image` 加 `webp` feature）、`src-tauri/src/flow.rs`、`src-tauri/src/commands.rs`、`src-tauri/src/lib.rs`
- Modify: `src/js/today.js`、`src/js/api.js`、`src/styles/main.css`
- Test: `import.rs` 单元测试；`$E/tcap.mjs` 的 `import` 模式

**Interfaces:**
- Consumes: Task 1 `Draft`；Task 3 `show_draft_note`；Task 4 的小条处理
- Produces:
  ```rust
  // import.rs
  pub const IMPORT_MAX_SIDE: u32 = 8000;
  pub fn is_image_path(p: &Path) -> bool;            // png/jpg/jpeg/bmp/webp，不分大小写
  pub fn natural_cmp(a: &str, b: &str) -> Ordering;  // 数字按数值比，字母不分大小写
  pub fn sort_paths(paths: &mut [PathBuf]);          // 按文件名 natural_cmp
  pub fn decode(bytes: &[u8]) -> Option<RgbaImage>;  // 认不出、0 字节 → None；成功后 fit_within
  pub fn fit_within(img: RgbaImage, max: u32) -> RgbaImage;
  pub fn outcome_message(over_limit: usize, unreadable: usize) -> Option<String>;
  // over>0 → "一条最多 9 张，多出来的没加"；unreadable>0 → "有 N 个文件不是图片或读不出来，没加"；都有就用"；"连起来

  // flow.rs
  /// 有草稿就往里加（收起小条、把小窗叫到前台）；没有就新建草稿：
  /// 日期按现在，补一句小窗放在主窗口那块屏（主窗口不在就放主屏），背后不变暗
  pub fn add_images(app: &AppHandle, imgs: Vec<RgbaImage>, unreadable: usize);
  ```
  命令：
  - `add_image_paths(paths: Vec<String>)`：拖进来的，先排序，再逐个读文件、`decode`。
  - `pick_images()`：异步；`blocking_pick_files` 选多个，过滤器"图片"；放 `spawn_blocking` 里，父窗口是当前调用的窗口。
  - `add_image_bytes(request: Request)`：粘贴的，body 是图片文件字节，同步命令，开线程处理。

- [ ] **Step 1: 写失败的单元测试**
  ```rust
  #[test] fn natural_sort_by_file_name() {
      // ["10.png","2.png","1.png","b.png","A10.png","a2.png"] → ["1.png","2.png","10.png","a2.png","A10.png","b.png"]
  }
  #[test] fn is_image_path_by_extension() { // a.PNG / b.jpeg / c.webp → true；d.txt / e → false
  }
  #[test] fn decode_png_and_webp() { // 用 image 生成 3×2 的 png 和无损 webp 字节，decode 后尺寸 3×2
  }
  #[test] fn decode_garbage_is_none() { // b"" 和 b"not an image" → None
  }
  #[test] fn fit_within_caps_long_side() { // 9000×100 → 8000×89；100×50 原样
  }
  #[test] fn outcome_messages() { // (0,0) → None；(2,0)；(0,1)；(1,3) 拼接
  }
  ```
- [ ] **Step 2: 跑测试确认失败**：`cd src-tauri && cargo test --lib import`
  Expected: 编译失败。
- [ ] **Step 3: 实现 `import.rs`、`flow::add_images` 和三个命令**
  - 存图：每张 `shot_id(now)` → `shots/{id}.png`，只存放得下的张数（`MAX_SHOTS - 已有`）。多出来的计入 `over_limit`，不写文件。
  - 新建草稿时 `id` = 第一张的 `shot_id`。
  - 草稿存盘后 `show_draft_note(app, false, outcome_message(...))`。
  - 主窗口保持显示，补一句小窗置顶在它前面。
- [ ] **Step 4: 写端到端测试 `import` 模式**
  1. 准备文件：在 `$E/imgs/` 下用 PowerShell `System.Drawing` 生成 `10.png`、`2.png`、`1.png`（尺寸分别 30×10、20×10、10×10），再写一个 `x.png` 内容是文本。
  2. 在主窗口执行 `add_image_paths([...四个路径])`。检查：补一句小窗出现，草稿 3 张，按存下的文件尺寸看顺序是 10、20、30 宽；小窗 `message` 含"有 1 个文件"。
  3. 再加 7 张（复用同一张图 7 次）。检查：草稿 9 张，`message` 含"一条最多 9 张"，`shots/` 文件数是 9。
  4. 用 `add_image_bytes` 发 `1.png` 的字节。检查：仍 9 张，`message` 同上。
  5. `note_submit('')`。检查：1 条，9 张，顺序不变。
  6. 主窗口"从图片添加"按钮存在（`.page-ft` 里文字是"从图片添加"）。对话框是系统窗口，不在自动测试里点。
- [ ] **Step 5: 实现"今天"页入口**
  - 页面底部按钮"从图片添加" → `invoke('pick_images')`。
  - 拖入：`window.__TAURI__.webview.getCurrentWebview().onDragDropEvent`：
    - `enter` 时显示遮罩 `.drop-mask`，文字"松开，用这 N 张图添加一条"（N = 图片扩展名的路径数），小字"支持 png / jpg，一次最多 9 张"；
    - `leave` 收起；
    - `drop` 收起，并 `invoke('add_image_paths', {paths})`。
  - 粘贴：`document` 的 `paste` 事件，焦点不在输入框时，取 `clipboardData.items` 里 `kind === 'file'` 且 `type` 以 `image/` 开头的；有就 `preventDefault`，逐个 `arrayBuffer()` 后 `invoke('add_image_bytes', new Uint8Array(buf))`。
- [ ] **Step 6: 跑测试确认通过**
  Run: `cd src-tauri && cargo test --lib && cd .. && npx tauri build --debug --no-bundle && bash $E/run_cap.sh import multi`
  Expected: 全部 PASS。
- [ ] **Step 7: 提交**：`git commit -m "feat: 已有图片拖入、粘贴、选文件加进一条（按文件名排序，最多 9 张）"`

### Task 8: 补一句小窗的多图界面与小条样式

**Files:**
- Modify: `src/note.html`、`src/js/note.js`、`src/bar.html`、`src/js/bar.js`
- Test: `$E/tcap.mjs` 的 `note-ui` 模式

**Interfaces:**
- Consumes: Task 3 `NoteOpen`；Task 4 命令 `note_more` / `note_text` / `note_blur` / `note_remove_shot` / `note_discard` / `bar_done`；Task 7 命令 `pick_images` / `add_image_paths` / `add_image_bytes`
- Produces：页面约定（测试用）
  - 缩略图 `.th`，序号 `.th .n`，删除 `.th .x`；
  - `#more`（再截一张）、`#pick`（加图片）、`#discard`（不记了）；
  - `#idx`（"今天第 N 条 · N 张图"）、`#msg`（提示语）。

- [ ] **Step 1: 写端到端测试 `note-ui`**
  1. 以 `main` 截一次并记录，再用 `add_image_paths` 加 2 张。
  2. 在 `note.html` 检查：
     - `.th` 有 3 个；
     - `#idx` 是"今天第 N 条 · 3 张图"；
     - `.th .x` 可见；`#msg` 有内容时可见，约 4 秒后消失。
  3. 点第 2 个 `.x`。检查：剩 2 个 `.th`，序号是 1、2。
  4. 在 `#text` 输入"界面里写的"，等 400ms。检查：`draft.json` 的 `note` 一致。
  5. 点 `#more`。检查：小窗不可见，小条可见，小条文字含"已截 2 张"和热键。
  6. 在小条页面点"不截了，去写说明"。检查：小窗可见，`#text` 的值仍是"界面里写的"。
  7. 用 `add_image_paths` 加到 9 张。检查：`#more` 和 `#pick` 有 `disabled`。
  8. 点 `#discard`。检查：没有新条目，`shots/` 里这些文件都不在了。
  9. 再记一张（只剩 1 张）。检查：`.th .x` 不显示。
- [ ] **Step 2: 跑测试确认失败**：`npx tauri build --debug --no-bundle && bash $E/run_cap.sh note-ui`
  Expected: FAIL（没有 `.th`）。
- [ ] **Step 3: 实现**
  - **`note.html` 的布局**，照 `multi-shot.html` 预览稿的 ① 号画面，从上到下：
    - 标题行：`这一条在干啥？` 加 `#idx`；
    - 缩略图一排：`.th` × N，然后是 `#more`（"＋ 再截一张"，小字"翻到下一屏再截"）和 `#pick`（"＋ 加图片…"，小字"也能 Ctrl+V / 拖进来"）；
    - 输入框（占位文字不变）；
    - 黄色提示 `截图里没有的信息（人名、地点、原因）请补在这里`；`#msg` 出现时盖住它，4 秒后恢复；
    - 底部：左边是 `#discard` 文字链接"不记了"和"Win+H 可语音输入"；右边是"跳过 Esc"和"记录 Enter"。
  - **`note.js` 的行为**
    - `open(p)`：按 `p.shots` 画缩略图（只有 1 张时隐藏 ×）。满 `p.max` 时两格加 `disabled`。输入框的值 = `p.note`。
    - 输入框 `input` 事件，300ms 防抖后调 `note_text`。
    - `window` 的 `blur` 事件调 `note_blur`。
    - 粘贴：同 Task 7 的做法。剪贴板里没有图片文件时不拦截，让文字照常粘贴。
    - 拖入：`onDragDropEvent` 的 `drop` 事件调 `add_image_paths`。
    - 回车、Esc 的处理不变。
  - **`bar.html` 的样式**照预览稿：深色半透明圆角条，按钮浅色。
- [ ] **Step 4: 跑测试确认通过**
  Run: `npx tauri build --debug --no-bundle && bash $E/run_cap.sh note-ui multi import record-one`
  Expected: 全部 PASS。
- [ ] **Step 5: 提交**：`git commit -m "feat: 补一句小窗显示多张缩略图，再截一张、加图片、粘贴、拖入、不记了"`

### Task 9: AI 一次看多张图

**Files:**
- Modify: `src-tauri/src/recognize.rs`、`src-tauri/src/ai/prompt.rs`、`src-tauri/src/ai/presets.rs`、`src-tauri/src/capture.rs`
- Modify: `src-tauri/src/settings_cmds.rs`：`test_connection` 调用的 `recognize::build_messages(…, Some(&jpeg))` 改成 `&[jpeg]`
- Modify: `$E/mock_ai.py`：识图请求里图片超过 6 张时，返回 400 `{"error":{"message":"Too many images"}}`
- Test: 各模块单元测试；`$E/tcap.mjs` 的 `ai-multi` 模式

**Interfaces:**
- Consumes: 草稿记下后的 `Item.shots`（Task 3、4、7）
- Produces:
  ```rust
  // presets.rs
  pub fn max_images(provider: &str) -> usize; // "zhipu" → 5，"kimi" → 4，其余 → 9

  // capture.rs
  pub fn group_sizes(n: usize, limit: usize) -> Vec<usize>; // n<=limit → [1; n]；否则分成 limit 组，大小差不超过 1，大的在前
  pub fn stack_vertical(imgs: &[RgbaImage]) -> RgbaImage;   // 宽 = 最宽那张，高 = 总和，左对齐，空白处填白

  // prompt.rs
  pub const ANNOTATION_NOTE: &str = "图上的框、圈、箭头、手写文字、马赛克可能是用户自己加的标注：标注指的地方是重点，不要描述标注本身，也不要猜马赛克下面的内容。";
  pub fn multi_image_note(n: usize, merged: bool) -> String;
  // "这次给你的是 {n} 张截图，是同一件事，按顺序看，合起来只写一条。"
  // merged 时再加一句 "有的图是几张截图上下拼在一起的，从上往下看。"
  pub fn vision_prompt(s: &Settings, images: usize, merged: bool) -> String;
  // 0 张 → 纯文字模板；≥1 → 识图模板（或用户改过的提示词）
  // ≥2 张 → 追加 multi_image_note；≥1 张 → 追加 ANNOTATION_NOTE；最后追加 VISION_TAIL
  // 段落之间用 "\n\n" 连接

  // recognize.rs
  pub fn build_messages(system: &str, note: &str, images: &[Vec<u8>]) -> Value; // 按顺序放所有 image_url，最后放文字
  pub fn vision_timeout(images: usize) -> Duration; // ≤1 → 20s；n → min(20 + 10*(n-1), 60) 秒
  pub fn is_too_many_images(e: &AiError) -> bool;
  // Http(400|413|422, msg) 且 msg 小写后包含 image/图片/too many/exceed/超过/超出/token/length/context 之一
  pub fn prepare_images(imgs: &[RgbaImage], limit: usize) -> (Vec<Vec<u8>>, bool /*merged*/); // 分组、拼图、ai_jpeg
  ```

- [ ] **Step 1: 写失败的单元测试**
  ```rust
  #[test] fn group_sizes_even_split() {
      assert_eq!(group_sizes(3, 9), vec![1, 1, 1]);
      assert_eq!(group_sizes(7, 5), vec![2, 2, 1, 1, 1]);
      assert_eq!(group_sizes(9, 4), vec![3, 2, 2, 2]);
      assert_eq!(group_sizes(9, 1), vec![9]);
  }
  #[test] fn stack_vertical_pads_white() { // 2×1 红 + 3×2 蓝 → 3×3；(2,0) 是白；(0,0) 红；(0,1) 蓝
  }
  #[test] fn max_images_per_preset() { // zhipu 5，kimi 4，qwen 9，custom 9，未知 9
  }
  #[test] fn vision_prompt_by_image_count() {
      // 0：不含 ANNOTATION_NOTE、不含"张截图"；1：含 ANNOTATION_NOTE、不含"这次给你的是"
      // 3：含"这次给你的是 3 张截图"和 ANNOTATION_NOTE，ends_with(VISION_TAIL)
      // 3 + merged：另含"上下拼在一起"
  }
  #[test] fn vision_prompt_override_still_gets_notes() { // prompt_overrides.vision = 自定义 → 2 张时仍含两段说明和结尾
  }
  #[test] fn build_messages_keeps_image_order() { // 3 张不同字节 → content 前 3 项是 image_url，按顺序；第 4 项是文字
  }
  #[test] fn vision_timeout_scales_and_caps() { // 0→20，1→20，2→30，5→60，9→60（秒）
  }
  #[test] fn too_many_images_detection() {
      // Http(400,"Too many images")、Http(400,"图片数量超过限制")、Http(413,"payload exceeds") → true
      // Http(401,"bad key")、Http(500,"image")、Timeout → false
  }
  ```
  现有用到 `vision_prompt(&s, true/false)`、`build_messages(.., Some(..))` 的测试一并改成新签名，断言意思不变。
- [ ] **Step 2: 跑测试确认失败**：`cd src-tauri && cargo test --lib`
  Expected: 编译失败。
- [ ] **Step 3: 实现，并改 `recognize::process`**
  - 读条目的全部 `shots`，不在的跳过。
  - 要发图且一张都读不出来、补充说明又是空 → 仍转待处理"截图文件读不出来"（沿用）。
  - `limit = max_images(&settings.provider)`；`prepare_images` 后发送，超时用 `vision_timeout(sent)`。
  - 出错时：
    - `is_too_many_images` 且 `sent > 1` → `limit = max(1, sent / 2)`，重新 `prepare_images` 后再发一次；
    - 否则按原来的 `should_retry` 规则。
  - 日志只记张数和拼图与否，不记图片内容。
- [ ] **Step 4: 写端到端测试 `ai-multi`**
  1. 3 张：用 `add_image_paths` 加 3 张后提交。检查 `requests.jsonl` 里最后一个识图请求：
     - 用户消息有 3 个 `image_url`，最后一项是文字；
     - 系统提示词含"这次给你的是 3 张截图"和 `ANNOTATION_NOTE`，结尾是 `VISION_TAIL`。
  2. 7 张：
     - 先把设置里的 `provider` 改成 `custom`（上限 9），加 7 张后提交；
     - 第一次请求带 7 张，被 mock 回 400；
     - 第二次请求带 3 张，系统提示词含"上下拼在一起"；
     - 条目最后是 `done`。
  3. 智谱：`provider` 改成 `zhipu`，加 7 张后提交。检查：第一次请求就只有 5 张。
- [ ] **Step 5: 跑测试确认通过**
  Run: `cd src-tauri && cargo test --lib && cd .. && npx tauri build --debug --no-bundle && bash $E/run_cap.sh ai-multi record-one`
  Expected: 全部 PASS。
- [ ] **Step 6: 提交**：`git commit -m "feat: 识图一次发多张图，超过服务商上限自动拼图，图多时放宽超时，告诉 AI 标注是用户加的"`

### Task 10: 清单、历史里的多图条目与大图翻页

**Files:**
- Modify: `src/js/itemlist.js`、`src/js/ui.js`、`src/styles/main.css`
- Create: `$E/fixture-3shots.json`：今天一条带 3 张图，图放在 `$E/fixture-shots/`。`run_app.sh` 支持第 4 个参数，把这个目录拷进 `data/shots`。
- Test: `$E/tcap.mjs` 的 `list` 模式

**Interfaces:**
- Consumes: `api.shotUrl`
- Produces:
  ```js
  // ui.js
  export function viewImages(urls, start = 0) // 多张时显示 ‹ ›、"2 / 3"，← → 翻页，Esc 或点背景关闭；一张时和原来一样
  // 原 viewImage(url) 改成 viewImages([url])，保留旧函数名做转发
  ```

- [ ] **Step 1: 写端到端测试 `list`**
  1. 用 `fixture-3shots` 启动。检查：今天页那条有 `.thumb-stack`，`.thumb-stack .cnt` 是"3"。
  2. 点 `.thumb-stack`。检查：`.viewer` 出现，`.viewer .count` 是"1 / 3"。
  3. 发 ArrowRight（`p.key` 加上 ArrowRight 的键码 39）。检查：变成"2 / 3"。
  4. 点 `.viewer .next` 两次。检查：停在"3 / 3"（不循环），`.next` 不可点。
  5. 按 Esc。检查：`.viewer` 消失。
  6. 只有一张图的条目仍是 `.thumb`，大图里没有 `.count`。
  7. 历史页同样显示 `.thumb-stack`。
  8. 删除和撤销（Spec 4.7）：
     - 删除这条后点"撤销"，检查 3 个图文件都还在；
     - 再删除并等 6 秒，检查 3 个图文件都被删掉了。
- [ ] **Step 2: 跑测试确认失败**：`bash $E/run_cap.sh list`
  Expected: FAIL。
- [ ] **Step 3: 实现**
  - **叠放缩略图**：两张 `img` 错开 3px 叠放，右下角蓝色小圆标写张数。
  - **图被清理时**：读不到的图同原来一样换成灰色占位；第一张读不到时，整摞换成占位，张数照标。
  - **大图翻页**：箭头放在图两侧，张数在底部居中，样式照 `multi-shot.html` 预览稿的 ④ 号画面。
- [ ] **Step 4: 跑测试确认通过**
  Run: `npx tauri build --debug --no-bundle && bash $E/run_cap.sh list`
  Expected: PASS。
- [ ] **Step 5: 提交**：`git commit -m "feat: 多图条目叠放显示张数，大图左右翻页"`

### Task 11: "框选后按回车"设置、首次引导、文案

**Files:**
- Modify: `src/js/settings.js`、`src/js/onboarding.js`、`src/js/itemlist.js`（空状态文案）、`src/js/guide.js`
- Modify: `docs/首次安装说明.md`、`README.md`
- Modify: `src-tauri/src/settings_cmds.rs`
- Test: `settings_cmds.rs` 单元测试；`$E/tcap.mjs` 的 `enter-setting` 模式；文案检查脚本

**Interfaces:**
- Consumes: Task 2 `EnterAction`、`Settings.enter_action`
- Produces:
  ```rust
  /// 走完引导时，没选过"框选后按回车"的按"复制"
  pub fn onboarding_defaults(s: &mut Settings); // enter_action 为 None 时设为 Some(Copy)，已有值不动
  ```
  `finish_onboarding` 在设 `onboarding_done = true` 前调它。

- [ ] **Step 1: 写失败的测试**
  ```rust
  #[test] fn onboarding_defaults_sets_copy_once() { // None → Some(Copy)；Some(Record) 保持 Record
  }
  ```
  端到端测试 `enter-setting`：
  1. 在设置页"使用习惯"找到"框选后按回车"那一行的 `select`，改成 `record`。
  2. 检查：`settings.json` 的 `enterAction === 'record'`；以 `hotkey` 截一次，`overlay_init().primary === 'record'`。
  3. 改回 `copy`，再截一次，`primary === 'copy'`。

  文案检查（写在 `tcap.mjs` 的 `copy-text` 模式里，不启动程序，直接读文件）：
  - `src/js/guide.js`、`docs/首次安装说明.md`、`README.md`、`src/js/onboarding.js` 都含"Tab"和"复制"。
  - `src/`、`docs/首次安装说明.md`、`README.md` 里没有"比如""例如""如："。
- [ ] **Step 2: 跑测试确认失败**：`cd src-tauri && cargo test --lib onboarding_defaults && cd .. && bash $E/run_cap.sh enter-setting copy-text`
  Expected: FAIL。
- [ ] **Step 3: 实现**
  - **设置页"使用习惯"**：在"截图快捷键"下面加一行"框选后按回车"。
    - 说明文字：`另一种操作：Tab 记录，Ctrl+C 复制`；
    - 控件：`select([{value:'copy', label:'复制'}, {value:'record', label:'记录（进补一句）'}], s.enterAction ?? 'copy', v => save({enterAction: v}))`。
  - **首次引导"最后几个习惯"**：
    - 导语改成：`做完一件事按快捷键截图，点「记录」（或按 Tab）记进来；下班前它会提醒你。`
    - 在快捷键下面加同样的一行。
  - **空状态**：`做完一件事按 {键} 截图，点「记录」记一条`。
  - **`guide.js` 和 `docs/首次安装说明.md`"平时怎么用"**，两处内容一致：
    1. `按截图快捷键（默认 Ctrl + Alt + S），拖动鼠标框选。框选后可以画圈、画箭头、打马赛克。`
    2. `按回车复制，可以直接粘贴给别人；按 Tab（或点「记录」）记进工作梳理。设置里可以把回车改成「记录」。`
    3. `记录时补一句话（截图里没有的人名、地点、原因写在这里），按回车。想把几张图记成一条，点「再截一张」，或把图片拖进来。`
    4. 原来"下班时……"那一条保留。
  - **README 第一段**：`常驻托盘的 Windows 小工具：按快捷键截图，能画圈、画箭头、打马赛克，回车复制；需要时点「记录」、补一句话，AI 生成一条工作记录（一条可以放多张图）；下班时一键生成格式固定的《{姓名}{月.日}工作梳理》，复制就能粘到微信。`
- [ ] **Step 4: 跑测试确认通过**
  Run: `cd src-tauri && cargo test --lib && cd .. && npm run test:js && npx tauri build --debug --no-bundle && bash $E/run_cap.sh enter-setting copy-text`
  Expected: 全部 PASS。
- [ ] **Step 5: 全量回归**
  Run: `bash $E/run_all.sh && bash $E/run_minor.sh && bash $E/run_cap.sh`
  Expected: 全部 PASS。旧测试里依赖"松手就出补一句"的地方，改成先按 Tab；这些改动记进台账。
- [ ] **Step 6: 提交**：`git commit -m "feat: 框选后按回车可选复制或记录，引导与说明文案更新"`

---

## 交接时需要需求方在真机上确认的（自动测试做不到）

1. 另存为、选文件这两个系统对话框能弹出，而且在框选画面和补一句小窗前面。另存一次后，下次另存的默认文件夹还是上次那个。
2. 复制出来的图能直接粘进微信、QQ、钉钉、Word。
3. 双屏不同缩放（150% + 100%）下，标注位置准确，"沿用上次的框"位置准确。
4. 只有一块屏时：补一句小窗切到资源管理器后变暗背景收起，能把文件拖到小窗上。
5. 用真实 API Key：
   - 3 张图识别正常；
   - 智谱、Kimi 一次 5 张、4 张不报错。这两个上限来自第三方资料，没有核实过。
6. 文字工具用中文输入法打字正常。
7. 自动测试里检查剪贴板时，会覆盖你当时剪贴板里的内容。
