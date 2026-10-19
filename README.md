# 每日工作梳理

常驻托盘的 Windows 小工具：做完一件事按快捷键截一张、补一句话，AI 生成一条工作记录；下班时一键生成格式固定的《{姓名}{月.日}工作梳理》，复制就能粘到微信。

- 使用说明：[docs/首次安装说明.md](docs/首次安装说明.md)
- 设计文档：[docs/superpowers/specs/2026-10-18-daily-worklog-design.md](docs/superpowers/specs/2026-10-18-daily-worklog-design.md)

## 开发

需要：Rust（MSVC 工具链）、Visual Studio 生成工具（C++）、Node.js 18+。

```bash
npm install
npm run dev          # 开发运行
cargo test --manifest-path src-tauri/Cargo.toml --lib   # 单元测试
npm run build        # 打包 NSIS 安装包，输出在 src-tauri/target/release/bundle/nsis/
```

测试时可以用环境变量 `DWL_DATA_DIR` 把数据目录指到别处，不影响真实数据。

## 结构

| 位置 | 内容 |
|---|---|
| `src/` | 界面（原生 HTML/CSS/JS，无打包工具） |
| `src-tauri/src/capture.rs` `flow.rs` | 冻结截屏、框选、补充说明、落盘 |
| `src-tauri/src/recognize.rs` `ai/` | 识图队列、OpenAI 兼容接口、提示词、回复解析 |
| `src-tauri/src/format.rs` `summary.rs` | 标题、格式清洗与检查、生成总结 |
| `src-tauri/src/reminder.rs` `holidays.rs` | 下班提醒、法定节假日 |
| `src-tauri/src/storage.rs` | 原子写入、备份恢复、截图清理 |
| `src-tauri/src/update.rs` | 检查 GitHub 最新 Release |

## 发布

1. 改 `src-tauri/tauri.conf.json` 里的 `version`，提交。
2. `git tag v0.1.0 && git push origin v0.1.0`。
3. GitHub Actions 自动打包并创建 Release。

可以在仓库 Settings → Variables 里设置 `DWL_PUBLISHER`（你的名字），它会显示在"GitHub 打不开？找 ×× 要安装包"里。

每年 12 月国务院公布下一年节假日后，把 [holiday-cn](https://github.com/NateScarlet/holiday-cn) 的新一年 JSON 放进 `src-tauri/resources/holidays/`，在 `holidays.rs` 里加一行，然后发个新版本。
