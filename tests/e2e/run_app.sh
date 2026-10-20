#!/usr/bin/env bash
# 用测试数据启动调试版程序（带远程调试端口 9222）。参数：数据夹具名 设置夹具名（fixtures/ 下，不含扩展名）
# 环境变量 KEEP_DATA=1：不清空数据目录（模拟崩溃后重开）
E="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(cd "$E/../.." && pwd)"
taskkill //IM DailyWorkLog.exe //F > /dev/null 2>&1
if [ -z "$KEEP_DATA" ]; then
  rm -rf "$E/data" && mkdir -p "$E/data"
  cp "$E/fixtures/${1:-data-empty}.json" "$E/data/data.json"
  cp "$E/fixtures/${2:-settings-v2}.json" "$E/data/settings.json"
fi
export DWL_DATA_DIR="$(cygpath -w "$E/data")"
export WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS="--remote-debugging-port=9222"
exec "$ROOT/src-tauri/target/debug/DailyWorkLog.exe"
