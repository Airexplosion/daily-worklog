#!/usr/bin/env bash
# 截图升级的端到端测试：bash tests/e2e/run_cap.sh [模式...]，不给模式就全跑。每个模式重启程序、换新测试数据。
# 先编译调试版：npx tauri build --debug --no-bundle。需要 Node 22、带 Pillow 的 Python、Windows。
# 注意：测试时框选窗口会真的全屏显示在屏幕上（别把鼠标移上去），剪贴板内容会被覆盖。
E="$(cd "$(dirname "$0")" && pwd)"
cd "$E"
mkdir -p out
rm -f requests.jsonl
python -I mock_ai.py requests.jsonl 18080 > /dev/null 2>&1 &
MOCK=$!
ALL="copy upgrade-hint record-one pixels multi discard crash toolbar annotate fast-annotate import note-ui ai-multi list enter-setting copy-text text-outside import-during-capture decode-fail full-nine note-typing-race second-screen focus-last-rect text-color bar-width paste-bad guide-follows-setting"
for m in ${@:-$ALL}; do
  st=settings-v2
  case $m in
    upgrade-hint) st=settings-v1 ;;
  esac
  bash run_app.sh data-empty "$st" > /dev/null 2>&1 &
  sleep 5
  echo "=== $m"
  out=$(timeout 180 node tcap.mjs "$E" "$m" 2>&1)
  echo "$out" | grep -E "PASS|FAIL|全部通过|项失败|Error|用了" | sed 's/^/  /'
  echo "$out" | grep -q "全部通过" || { BAD="$BAD $m"; echo "$out" | tail -4 | sed "s/^/  ! /"; cp data/logs/app.log "out/fail-$m.log" 2>/dev/null; }
  taskkill //IM DailyWorkLog.exe //F > /dev/null 2>&1
  sleep 1
done
kill $MOCK 2>/dev/null
if [ -n "$BAD" ]; then echo "失败的场景：$BAD"; exit 1; fi
echo "场景全部通过"
