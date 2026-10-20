// 截图升级的端到端检查：node tcap.mjs <e2e目录> <模式>（一般用 run_cap.sh 跑）
import fs from 'node:fs';
import { execSync } from 'node:child_process';
import { connect, check, failures, sleep } from './cdp.mjs';

const E = process.argv[2];
const mode = process.argv[3];
const D = `${E}/data`;
const readJson = (f) => JSON.parse(fs.readFileSync(f, 'utf8').replace(/^﻿/, ''));
const data = () => readJson(`${D}/data.json`);
const settings = () => readJson(`${D}/settings.json`);
const draft = () => (fs.existsSync(`${D}/draft.json`) ? readJson(`${D}/draft.json`) : null);
const shotFiles = () => (fs.existsSync(`${D}/shots`) ? fs.readdirSync(`${D}/shots`).filter((f) => f.endsWith('.png')) : []);
const reqs = () => (fs.existsSync(`${E}/requests.jsonl`) ? fs.readFileSync(`${E}/requests.jsonl`, 'utf8').split('\n').filter(Boolean).map((l) => JSON.parse(l)) : []);
const inv = (p, cmd, args = {}) => p.eval(`window.__TAURI__.core.invoke(${JSON.stringify(cmd)}, ${JSON.stringify(args)})`);
const visible = (p) => p.eval(`window.__TAURI__.window.getCurrentWindow().isVisible()`);
const pix = (file, ...pts) => JSON.parse(execSync(`python -I "${E}/pix.py" "${file}" ${pts.map((p) => p.join(',')).join(' ')}`, { encoding: 'utf8' }));
function clipboardSize() {
  return execSync(`powershell -NoProfile -STA -Command "Add-Type -AssemblyName System.Windows.Forms; $i=[Windows.Forms.Clipboard]::GetImage(); if ($i) { '' + $i.Width + 'x' + $i.Height } else { 'none' }"`, { encoding: 'utf8' }).trim();
}

const main = await connect('http://tauri.localhost/');
await main.waitFor(`!!document.querySelector('.nav')`, 8000);
const today = await inv(main, 'today');
const todayItems = () => data().days?.[today]?.items ?? [];

/** 开始一次截图，返回框选页和它的 CaptureInfo */
async function capture(origin) {
  await inv(main, 'capture_start', { origin });
  await sleep(origin === 'main' ? 900 : 600);
  const ov = await connect('capture.html?i=0');
  await ov.waitFor(`document.querySelector('#bg')?.classList.contains('ready')`, 5000);
  const info = await inv(ov, 'overlay_init', { index: 0 });
  return { ov, info };
}

async function waitDone(id, ms = 15000) {
  const end = Date.now() + ms;
  while (Date.now() < end) {
    const it = todayItems().find((i) => i.id === id);
    if (it && it.status !== 'recognizing') return it;
    await sleep(200);
  }
  return todayItems().find((i) => i.id === id);
}

if (mode === 'copy') {
  const files = shotFiles().length;
  const lines = reqs().length;
  const { ov, info } = await capture('hotkey');
  check('热键截图的主按钮是复制', info?.primary === 'copy', JSON.stringify(info));
  await inv(ov, 'capture_done', { index: 0, x: 0, y: 0, w: 200, h: 100, action: 'copy' });
  await sleep(700);
  check('剪贴板里是 200×100 的图', clipboardSize() === '200x100', clipboardSize());
  check('复制不在数据目录留图', shotFiles().length === files, String(shotFiles().length));
  check('复制不发给 AI', reqs().length === lines);
  check('复制后没有草稿', draft() === null);
  const toast = await connect('toast.html');
  check('右下角闪一下"已复制"', (await toast.eval(`document.querySelector('#title').textContent`)).includes('已复制'));
  check('框选窗口收起了', !(await visible(ov)));
  toast.close();
  ov.close();
}

if (mode === 'upgrade-hint') {
  const m = await capture('main');
  check('主窗口截图（主按钮是记录）不给升级提示', m.info?.hint === null, JSON.stringify(m.info));
  await inv(m.ov, 'capture_cancel');
  await sleep(600);
  const first = await capture('hotkey');
  check('老用户第一次热键截图有升级提示', typeof first.info?.hint === 'string' && first.info.hint.includes('Tab'), JSON.stringify(first.info));
  await first.ov.key('Escape');
  await sleep(500);
  check('还没框选就按 Esc：提示没显示过，不算数', settings().enterAction === undefined || settings().enterAction === null, String(settings().enterAction));
  const again = await capture('hotkey');
  check('下次热键截图还提示', typeof again.info?.hint === 'string', JSON.stringify(again.info));
  await drag(again.ov, [200, 200], [500, 400]);
  await sleep(300);
  const shownHint = await again.ov.eval(`document.querySelector('#hint').textContent`);
  check('框选后提示行显示升级说明', shownHint.includes('现在回车是复制'), shownHint);
  check('显示过以后设置里记下了"复制"', settings().enterAction === 'copy', String(settings().enterAction));
  await again.ov.key('Escape');
  await sleep(500);
  const third = await capture('hotkey');
  check('之后不再提示', third.info?.hint === null, JSON.stringify(third.info));
  await inv(third.ov, 'capture_cancel');
  first.ov.close();
}

if (mode === 'record-one') {
  const { ov, info } = await capture('main');
  check('主窗口截图的主按钮是记录', info?.primary === 'record', JSON.stringify(info));
  await inv(ov, 'capture_done', { index: 0, x: 10, y: 10, w: 300, h: 150, action: 'record' });
  await sleep(600);
  const note = await connect('note.html');
  const cur = await inv(note, 'note_current');
  check('补一句小窗带 1 张图', cur?.shots?.length === 1, JSON.stringify(cur));
  check('补一句小窗显示了', await visible(note));
  check('草稿已存盘', draft()?.shots?.length === 1, JSON.stringify(draft()));
  await inv(note, 'note_submit', { text: '测试一句' });
  await sleep(500);
  const items = todayItems();
  check('记下一条，带 1 张图', items.length === 1 && items[0].shots.length === 1, JSON.stringify(items));
  check('记下后草稿删掉了', draft() === null);
  const it = await waitDone(items[0]?.id);
  check('识别完成', it?.status === 'done', JSON.stringify(it));
  note.close();
  ov.close();
}

if (mode === 'pixels') {
  const { ov } = await capture('main');
  await ov.eval(`window.__TAURI__.core.invoke('capture_pixels', new Uint8Array([255,0,0,255, 255,0,0,255, 255,0,0,255, 255,0,0,255]),
    { headers: { 'x-index': '0', 'x-width': '2', 'x-height': '2', 'x-action': 'record' } })`);
  await sleep(800);
  const d = draft();
  check('像素直接记进草稿', d?.shots?.length === 1, JSON.stringify(d));
  if (d) {
    const r = pix(`${D}/${d.shots[0]}`, [0, 0]);
    check('存下的图是 2×2 红色', r.w === 2 && r.h === 2 && r.px[0][0] === 255 && r.px[0][1] === 0, JSON.stringify(r));
  }
  ov.close();
}

/** 以 main 截一张并记录，返回补一句小窗页面 */
async function recordOne(rect = { x: 100, y: 100, w: 300, h: 200 }) {
  const { ov } = await capture('main');
  await inv(ov, 'capture_done', { index: 0, ...rect, action: 'record' });
  await sleep(600);
  ov.close();
  return connect('note.html');
}

if (mode === 'multi') {
  const note = await recordOne();
  await inv(note, 'note_text', { text: '留着的字' });
  await sleep(200);
  check('打的字存进草稿', draft()?.note === '留着的字', JSON.stringify(draft()));
  await inv(note, 'note_more');
  await sleep(500);
  const bar = await connect('bar.html');
  check('点再截一张：小窗收起', !(await visible(note)));
  check('点再截一张：小条出现', await visible(bar));
  let { ov, info } = await capture('hotkey');
  check('有草稿时主按钮是加到这条', info?.primary === 'add', JSON.stringify(info));
  check('是第 2 张', info?.shotNo === 2);
  check('上次的框摆回原位', JSON.stringify(info?.lastRect) === JSON.stringify({ x: 100, y: 100, w: 300, h: 200 }), JSON.stringify(info?.lastRect));
  check('截图时小条收起', !(await visible(bar)));
  await inv(ov, 'capture_done', { index: 0, x: 100, y: 100, w: 300, h: 200, action: 'add' });
  await sleep(600);
  check('加到这条后 2 张', draft()?.shots?.length === 2, JSON.stringify(draft()));
  check('回到补一句小窗', await visible(note));
  check('打的字还在', (await inv(note, 'note_current'))?.note === '留着的字');
  ({ ov } = await capture('hotkey'));
  await inv(ov, 'capture_cancel');
  await sleep(500);
  check('加到这条时按 Esc：回到小窗', await visible(note));
  check('按 Esc 不丢图', draft()?.shots?.length === 2);
  ({ ov } = await capture('hotkey'));
  await inv(ov, 'capture_done', { index: 0, x: 0, y: 0, w: 50, h: 50, action: 'copy' });
  await sleep(600);
  check('加到这条时复制：回到小窗', await visible(note));
  check('复制不加图、不留文件', draft()?.shots?.length === 2 && shotFiles().length === 2, String(shotFiles().length));
  ({ ov, info } = await capture('hotkey'));
  check('小窗开着时按热键：小窗收起、进加到这条', !(await visible(note)) && info?.primary === 'add');
  await inv(ov, 'capture_cancel');
  await sleep(500);
  const first = draft().shots[0];
  await inv(note, 'note_remove_shot', { index: 0 });
  await sleep(300);
  check('去掉第一张', draft()?.shots?.length === 1 && !fs.existsSync(`${D}/${first}`), JSON.stringify(draft()));
  ({ ov } = await capture('hotkey'));
  await inv(ov, 'capture_done', { index: 0, x: 10, y: 10, w: 60, h: 40, action: 'record' });
  await sleep(600);
  const order = draft().shots;
  check('Tab（记录）在有草稿时也是加到这条', order.length === 2);
  await inv(note, 'note_submit', { text: '留着的字' });
  await sleep(500);
  const items = todayItems();
  check('记成一条 2 张图，顺序不变', items.length === 1 && JSON.stringify(items[0].shots) === JSON.stringify(order), JSON.stringify(items));
  check('记下后草稿删掉', draft() === null);
  ov.close();
  bar.close();
  note.close();
}

if (mode === 'discard') {
  const lines = reqs().length;
  const note = await recordOne();
  await inv(note, 'note_discard');
  await sleep(500);
  check('不记了：没有新条目', todayItems().length === 0);
  check('不记了：图删掉', shotFiles().length === 0, String(shotFiles()));
  check('不记了：草稿删掉', draft() === null);
  check('不记了：不发 AI', reqs().length === lines);
  check('不记了：小窗收起', !(await visible(note)));
  note.close();
}

if (mode === 'crash') {
  const note = await recordOne();
  await inv(note, 'note_text', { text: '崩溃前的字' });
  await inv(note, 'note_more');
  await sleep(300);
  note.close();
  main.close();
  execSync('taskkill /IM DailyWorkLog.exe /F');
  await sleep(1000);
  const { spawn } = await import('node:child_process');
  spawn('bash', [`${E}/run_app.sh`], { env: { ...process.env, KEEP_DATA: '1' }, detached: true, stdio: 'ignore' }).unref();
  await sleep(5000);
  const again = await connect('http://tauri.localhost/');
  await again.waitFor(`!!document.querySelector('.nav')`, 8000);
  const items = todayItems();
  check('重开后记成一条，带图和字', items.length === 1 && items[0].shots.length === 1 && items[0].note === '崩溃前的字', JSON.stringify(items));
  check('重开后草稿删掉', draft() === null);
  const it = await waitDone(items[0]?.id);
  check('重开后自动识别完成', it?.status === 'done', JSON.stringify(it));
  again.close();
}

/** 在框选页里按住左键从 a 拖到 b（CSS 像素，只发给网页，不动系统鼠标） */
async function drag(p, a, b, steps = 6) {
  await p.send('Input.dispatchMouseEvent', { type: 'mouseMoved', x: a[0], y: a[1] });
  await p.send('Input.dispatchMouseEvent', { type: 'mousePressed', x: a[0], y: a[1], button: 'left', buttons: 1, clickCount: 1 });
  for (let i = 1; i <= steps; i++) {
    const x = a[0] + ((b[0] - a[0]) * i) / steps, y = a[1] + ((b[1] - a[1]) * i) / steps;
    await p.send('Input.dispatchMouseEvent', { type: 'mouseMoved', x, y, button: 'left', buttons: 1 });
  }
  await p.send('Input.dispatchMouseEvent', { type: 'mouseReleased', x: b[0], y: b[1], button: 'left', buttons: 0, clickCount: 1 });
  await sleep(150);
}
async function mouseAt(p, x, y, button = 'left', clickCount = 1) {
  await p.send('Input.dispatchMouseEvent', { type: 'mouseMoved', x, y });
  for (let i = 1; i <= clickCount; i++) {
    await p.send('Input.dispatchMouseEvent', { type: 'mousePressed', x, y, button, clickCount: i });
    await p.send('Input.dispatchMouseEvent', { type: 'mouseReleased', x, y, button, clickCount: i });
  }
  await sleep(150);
}
const shown = (p, s) => p.eval(`(() => { const e = document.querySelector(${JSON.stringify(s)}); return !!e && !e.hidden && getComputedStyle(e).display !== 'none'; })()`);

if (mode === 'toolbar') {
  let { ov } = await capture('hotkey');
  await drag(ov, [200, 200], [600, 500]);
  check('松手后出工具栏', await shown(ov, '#toolbar'));
  const acts = await ov.eval(`[...document.querySelectorAll('#toolbar .act')].map(b => b.dataset.act + (b.classList.contains('primary') ? '*' : '')).join(',')`);
  check('按钮顺序：保存、记录、复制（主）', acts === 'save,record,copy*', acts);
  const hint = await ov.eval(`document.querySelector('#hint').textContent`);
  check('框选后的提示行', hint === 'Enter 复制　Tab 记录　Ctrl+S 保存　Esc 取消', hint);
  const before = await ov.eval(`document.querySelector('#size').textContent`);
  await drag(ov, [600, 500], [700, 560]);
  const after = await ov.eval(`document.querySelector('#size').textContent`);
  check('拖右下角能调大小', before !== after && after === '750 × 540', `${before} → ${after}`);
  await ov.key('Tab');
  await sleep(700);
  const note = await connect('note.html');
  check('Tab 记录：进补一句小窗', await visible(note));
  const d1 = draft();
  check('草稿 1 张', d1?.shots?.length === 1, JSON.stringify(d1));
  if (d1) {
    const r = pix(`${D}/${d1.shots[0]}`);
    check('图的尺寸 = 选框物理尺寸', r.w === 750 && r.h === 540, `${r.w}x${r.h}`);
  }
  await inv(note, 'note_more');
  await sleep(400);
  let info2;
  ({ ov, info: info2 } = await capture('hotkey'));
  await sleep(300);
  const okFrame = await shown(ov, '#sel') && await shown(ov, '#toolbar');
  check('再截一张：框已摆好、工具栏出来', okFrame, okFrame ? '' : JSON.stringify({ info2, ready: await ov.eval("document.querySelector('#bg').className"), sel: await ov.eval("getComputedStyle(document.querySelector('#sel')).display"), body: await ov.eval('document.body.className'), url: await ov.eval('location.href') }));
  const addBtn = await ov.eval(`document.querySelector('#toolbar [data-act].primary')?.textContent || ''`);
  check('主按钮是"加到这条（第 2 张）"', addBtn.includes('加到这条（第 2 张）'), addBtn);
  check('提示行写已截 1 张', (await ov.eval(`document.querySelector('#hint').textContent`)).includes('已截 1 张'));
  await ov.key('Enter');
  await sleep(700);
  check('回车加进这条：2 张', draft()?.shots?.length === 2);
  ({ ov } = await capture('hotkey'));
  await sleep(300);
  await mouseAt(ov, 450, 380, 'left', 2);
  await sleep(700);
  check('双击选框 = 主按钮：3 张', draft()?.shots?.length === 3);
  ({ ov } = await capture('hotkey'));
  await sleep(300);
  await mouseAt(ov, 450, 380, 'right');
  check('右键清掉选框', !(await shown(ov, '#sel')));
  await ov.key('Escape');
  await sleep(600);
  check('再按 Esc 回到补一句小窗', await visible(note));
  check('还是 3 张', draft()?.shots?.length === 3, JSON.stringify(draft()?.shots));
  note.close();
  ov.close();
}

const shapeCount = (ov) => ov.eval(`window.__shapes.length`);
async function pickTool(ov, tool) {
  await ov.waitFor(`!document.querySelector('[data-tool="${tool}"]').disabled`, 4000);
  await ov.click(`[data-tool="${tool}"]`);
}

if (mode === 'annotate') {
  let { ov } = await capture('main');
  await drag(ov, [200, 200], [600, 500]);
  await pickTool(ov, 'ellipse');
  await drag(ov, [250, 250], [350, 330]);
  await pickTool(ov, 'mosaic');
  await ov.click('#style [data-size="2"]');
  await drag(ov, [400, 220], [560, 300]);
  check('画了椭圆和马赛克', await shapeCount(ov) === 2);
  check('画了以后拖动点隐藏', await ov.eval(`document.body.classList.contains('locked')`));
  await pickTool(ov, 'text');
  await mouseAt(ov, 260, 420);
  await ov.waitFor(`document.activeElement?.id === 'textin'`, 2000);
  await ov.type('标注');
  await ov.key('Enter');
  await sleep(300);
  check('打字时回车只结束文字，不记录', draft() === null);
  check('文字上了画布', await shapeCount(ov) === 3);
  await ov.combo('z', 'KeyZ', 90, 2);
  check('Ctrl+Z 撤销文字', await shapeCount(ov) === 2);
  await mouseAt(ov, 260, 420);
  await ov.waitFor(`document.activeElement?.id === 'textin'`, 2000);
  await ov.type('标注');
  await ov.key('Enter');
  const t0 = Date.now();
  await ov.key('Tab');
  while (!draft() && Date.now() - t0 < 6000) await sleep(50);
  console.log(`按 Tab 到记下用了 ${Date.now() - t0}ms（画面已就绪）`);
  await sleep(300);
  const d = draft();
  check('Tab 记录了带标注的图', d?.shots?.length === 1, JSON.stringify(d));
  if (d) {
    // 选框 CSS (200,200)-(600,500)，缩放 1.5 → 600×450；马赛克 CSS (400,220)-(560,300) → 选区内物理 (300,30)-(540,150)，格子 22×1.5=33
    const r = pix(`${D}/${d.shots[0]}`, [75, 135], [305, 35], [325, 55], [0, 0]);
    check('图的尺寸是选框物理尺寸', r.w === 600 && r.h === 450, `${r.w}x${r.h}`);
    check('椭圆边上是红色', r.px[0][0] > 200 && r.px[0][1] < 60, JSON.stringify(r.px[0]));
    check('马赛克同一块里颜色一样', JSON.stringify(r.px[1]) === JSON.stringify(r.px[2]), JSON.stringify(r.px));
    check('图不透明', r.px[3][3] === 255);
  }
  const note = await connect('note.html');
  await inv(note, 'note_more');
  await sleep(400);
  ({ ov } = await capture('hotkey'));
  await sleep(300);
  await pickTool(ov, 'mosaic');
  check('马赛克档位记住了', await ov.eval(`document.querySelector('#style [data-size="2"]').classList.contains('on')`));
  await ov.key('Escape');
  note.close();
  ov.close();
}

if (mode === 'fast-annotate') {
  await inv(main, 'capture_start', { origin: 'main' });
  await sleep(400);
  const ov = await connect('capture.html?i=0');
  // 框选窗口显示出来（收到这次截图）就动手，不等定格画面加载完
  await ov.waitFor(`!!document.querySelector('#bg').getAttribute('src')`, 5000);
  const early = await ov.eval(`!document.querySelector('#bg').classList.contains('ready')`);
  console.log('画面还没就绪就开始画：' + early);
  await drag(ov, [300, 300], [700, 600], 2);
  await ov.eval(`document.querySelector('[data-tool="rect"]').click()`);
  await drag(ov, [350, 350], [500, 450], 2);
  const t0 = Date.now();
  await ov.key('Tab');
  while (!draft() && Date.now() - t0 < 6000) await sleep(50);
  console.log(`按 Tab 到记下用了 ${Date.now() - t0}ms（画面就绪前开始：${early}）`);
  const d = draft();
  check('手快也能记录', d?.shots?.length === 1, JSON.stringify(d));
  if (d) {
    const dim = pix(`${D}/${d.shots[0]}`);
    check('手快时图的尺寸也对', dim.w === 600 && dim.h === 450, `${dim.w}x${dim.h}`);
    const r = pix(`${D}/${d.shots[0]}`, [5, 5], [Math.min(300, dim.w - 1), Math.min(200, dim.h - 1)]);
    check('图不是全透明', r.px[0][3] === 255 && r.px[1][3] === 255, JSON.stringify(r));
    check('图不是纯黑', r.px.some((p) => p[0] + p[1] + p[2] > 0), JSON.stringify(r));
    check('手快画的框也在图上', r.px[1][0] > 200 && r.px[1][1] < 60, JSON.stringify(r.px[1]));
  }
  ov.close();
}

/** 在 $E/imgs 下生成测试图片：name → [宽, 高]；内容是文字的假图片 */
function makeImages(spec, fake = []) {
  const dir = `${E}/imgs`;
  fs.rmSync(dir, { recursive: true, force: true });
  fs.mkdirSync(dir, { recursive: true });
  const py = Object.entries(spec).map(([n, [w, h]]) => `Image.new('RGB', (${w}, ${h}), (40, 120, 200)).save(r'${dir}/${n}')`).join('; ');
  execSync(`python -I -c "from PIL import Image; ${py}"`);
  for (const n of fake) fs.writeFileSync(`${dir}/${n}`, '这不是图片');
  return (n) => `${dir}/${n}`.replace(/\//g, '\\');
}

if (mode === 'import') {
  const at = makeImages({ '10.png': [30, 10], '2.png': [20, 10], '1.png': [10, 10] }, ['x.png']);
  const msg = await inv(main, 'add_image_paths', { paths: [at('10.png'), at('x.png'), at('2.png'), at('1.png')] });
  await sleep(800);
  const note = await connect('note.html');
  check('拖进来后补一句小窗出现', await visible(note));
  const d = draft();
  check('3 张进了草稿', d?.shots?.length === 3, JSON.stringify(d));
  if (d) {
    const widths = d.shots.map((s) => pix(`${D}/${s}`).w);
    check('按文件名排：1、2、10', JSON.stringify(widths) === '[10,20,30]', JSON.stringify(widths));
  }
  check('有草稿时不返回提示（提示在小窗里）', msg === null, JSON.stringify(msg));
  const cur = await inv(note, 'note_current');
  check('小窗收到加图结果', cur?.shots?.length === 3);
  const lastMsg = await note.eval(`document.querySelector('#msg')?.textContent ?? ''`);
  console.log('小窗提示：', lastMsg);
  await inv(main, 'add_image_paths', { paths: Array(7).fill(at('1.png')) });
  await sleep(800);
  check('加满 9 张为止', draft()?.shots?.length === 9 && shotFiles().length === 9, `${draft()?.shots?.length} / ${shotFiles().length}`);
  const png = fs.readFileSync(`${E}/imgs/1.png`);
  await main.eval(`window.__TAURI__.core.invoke('add_image_bytes', new Uint8Array(${JSON.stringify([...png])}))`);
  await sleep(800);
  check('满了再粘贴也不加', draft()?.shots?.length === 9 && shotFiles().length === 9);
  const order = draft().shots;
  await inv(note, 'note_submit', { text: '' });
  await sleep(500);
  const items = todayItems();
  check('记成一条 9 张，顺序不变', items.length === 1 && JSON.stringify(items[0].shots) === JSON.stringify(order), JSON.stringify(items.map((i) => i.shots.length)));
  const bad = await inv(main, 'add_image_paths', { paths: [at('x.png')] });
  check('全是读不出的文件：返回提示，不开小窗', typeof bad === 'string' && bad.includes('有 1 个文件') && draft() === null, JSON.stringify(bad));
  const btn = await main.eval(`[...document.querySelectorAll('.page-ft .btn')].some(b => b.textContent.includes('从图片添加'))`);
  check('今天页有"从图片添加"按钮', btn);
  note.close();
}

if (mode === 'note-ui') {
  const at = makeImages({ 'a.png': [40, 20], 'b.png': [50, 20] });
  const note = await recordOne();
  await inv(main, 'add_image_paths', { paths: [at('a.png'), at('b.png')] });
  await sleep(700);
  await note.waitFor(`document.querySelectorAll('.th').length === 3`, 3000);
  check('3 张缩略图', await note.eval(`document.querySelectorAll('.th').length`) === 3);
  const idx = await note.eval(`document.querySelector('#idx').textContent`);
  check('标题行写 3 张图', idx === '今天第 1 条 · 3 张图', idx);
  check('多张时有 ×', await shown(note, '.th .x'));
  const second = draft().shots[1];
  await note.click('.th:nth-child(2) .x');
  await sleep(500);
  const nums = await note.eval(`[...document.querySelectorAll('.th .n')].map(n => n.textContent).join(',')`);
  check('去掉第 2 张后剩 2 张，序号 1、2', nums === '1,2' && draft().shots.length === 2 && !fs.existsSync(`${D}/${second}`), nums);
  await note.eval(`document.querySelector('#text').focus()`);
  await note.type('界面里写的');
  await sleep(500);
  check('打的字存进草稿', draft()?.note === '界面里写的', JSON.stringify(draft()?.note));
  await note.click('#more');
  await sleep(500);
  const bar = await connect('bar.html');
  check('点再截一张：小窗收起、小条出现', !(await visible(note)) && await visible(bar));
  const barText = await bar.eval(`document.querySelector('#text').textContent`);
  check('小条写已截 2 张和热键', barText.includes('已截 2 张') && barText.includes('Ctrl') && barText.includes('第 3 张'), barText);
  await bar.click('#done');
  await sleep(600);
  check('不截了：小窗回来', await visible(note));
  check('字还在', await note.eval(`document.querySelector('#text').value`) === '界面里写的');
  await inv(main, 'add_image_paths', { paths: Array(8).fill(at('a.png')) });
  await sleep(800);
  check('满 9 张：两格变灰', await note.eval(`document.querySelector('#more').disabled && document.querySelector('#pick').disabled`));
  const msg = await note.eval(`document.querySelector('#msg').textContent`);
  check('多出来的有提示', msg.includes('一条最多 9 张') && await shown(note, '#msg'), msg);
  const files = draft().shots;
  await note.click('#discard');
  await sleep(600);
  check('不记了：没有条目、图都删了', todayItems().length === 0 && files.every((f) => !fs.existsSync(`${D}/${f}`)) && draft() === null);
  const note2 = await recordOne();
  await note2.waitFor(`document.querySelectorAll('.th').length === 1`, 3000);
  check('只有 1 张时没有 ×', !(await shown(note2, '.th .x')));
  check('只有 1 张时标题不写张数', (await note2.eval(`document.querySelector('#idx').textContent`)) === '今天第 1 条');
  bar.close();
  note2.close();
}

const visionReqs = () => reqs().filter((r) => !r.body.stream && JSON.stringify(r.body.messages[0]).includes('只输出 JSON'));
const imageCount = (r) => (Array.isArray(r.body.messages[1].content) ? r.body.messages[1].content.filter((p) => p.type === 'image_url').length : 0);
async function addAndSubmit(at, names) {
  await inv(main, 'add_image_paths', { paths: names.map(at) });
  await sleep(700);
  const note = await connect('note.html');
  await inv(note, 'note_submit', { text: '' });
  note.close();
  await sleep(300);
  return todayItems().at(-1);
}

if (mode === 'ai-multi') {
  const at = makeImages(Object.fromEntries(Array.from({ length: 7 }, (_, i) => [`p${i + 1}.png`, [60 + i * 10, 40]])));
  const before = visionReqs().length;
  let it = await addAndSubmit(at, ['p1.png', 'p2.png', 'p3.png']);
  it = await waitDone(it.id);
  let vr = visionReqs().slice(before);
  check('3 张：一次请求', vr.length === 1, String(vr.length));
  if (vr[0]) {
    const content = vr[0].body.messages[1].content;
    check('3 张：用户消息里 3 个图，最后是文字', imageCount(vr[0]) === 3 && content.at(-1).type === 'text');
    const sys = vr[0].body.messages[0].content;
    check('3 张：提示词说了张数和标注', sys.includes('这次给你的是 3 张截图') && sys.includes('用户自己加的标注') && sys.endsWith('"confidence":0.0}'), sys.slice(-200));
  }
  check('3 张：识别完成', it?.status === 'done', JSON.stringify(it?.status));

  const view = await inv(main, 'get_settings');
  await inv(main, 'save_settings', { patch: { provider: 'custom' } });
  const n0 = visionReqs().length;
  it = await addAndSubmit(at, ['p1.png', 'p2.png', 'p3.png', 'p4.png', 'p5.png', 'p6.png', 'p7.png']);
  it = await waitDone(it.id, 20000);
  vr = visionReqs().slice(n0);
  check('7 张（上限 9）：先发 7 张被拒，再拼成 3 张', vr.length === 2 && imageCount(vr[0]) === 7 && imageCount(vr[1]) === 3, vr.map(imageCount).join(','));
  check('拼图后提示词说了拼在一起', vr[1]?.body.messages[0].content.includes('拼在一起'));
  check('7 张：最后识别完成', it?.status === 'done', JSON.stringify(it?.status));

  await inv(main, 'save_settings', { patch: { provider: 'zhipu' } });
  const n1 = visionReqs().length;
  it = await addAndSubmit(at, ['p1.png', 'p2.png', 'p3.png', 'p4.png', 'p5.png', 'p6.png', 'p7.png']);
  it = await waitDone(it.id, 20000);
  vr = visionReqs().slice(n1);
  check('智谱：第一次就只发 5 张', vr.length === 1 && imageCount(vr[0]) === 5, vr.map(imageCount).join(','));
  await inv(main, 'save_settings', { patch: { provider: view.settings.provider } });
}

if (mode === 'list') {
  const at = makeImages({ 'q1.png': [60, 40], 'q2.png': [70, 40], 'q3.png': [80, 40] });
  const multi = await addAndSubmit(at, ['q1.png', 'q2.png', 'q3.png']);
  const one = await recordOne();
  await inv(one, 'note_submit', { text: '' });
  one.close();
  await sleep(800);
  await main.waitFor(`document.querySelectorAll('.item').length === 2`, 5000);
  check('多图条目叠放显示', await main.eval(`!!document.querySelector('.thumb-stack')`));
  check('角标写 3', (await main.eval(`document.querySelector('.thumb-stack .cnt')?.textContent`)) === '3');
  check('单张条目还是普通缩略图', await main.eval(`document.querySelectorAll('img.thumb:not(.back):not(.front)').length === 1`));
  await main.click('.thumb-stack');
  await sleep(300);
  const count = () => main.eval(`document.querySelector('.viewer .count')?.textContent ?? ''`);
  check('点开大图：1 / 3', (await count()) === '1 / 3', await count());
  await main.combo('ArrowRight', 'ArrowRight', 39, 0);
  check('→ 键翻到 2 / 3', (await count()) === '2 / 3', await count());
  await main.click('.viewer .next');
  await main.click('.viewer .next');
  check('翻到最后停在 3 / 3，下一张不可点', (await count()) === '3 / 3' && await main.eval(`document.querySelector('.viewer .next').disabled`), await count());
  await main.key('Escape');
  check('Esc 关大图', await main.eval(`!document.querySelector('.viewer')`));
  await main.click('img.thumb:not(.back):not(.front)');
  await sleep(300);
  check('单张大图没有张数', await main.eval(`!!document.querySelector('.viewer') && !document.querySelector('.viewer .count')`));
  await main.key('Escape');
  await main.click('.nav button[data-page="history"]');
  await main.waitFor(`!!document.querySelector('.days .day')`, 4000);
  await main.click('.days .day');
  check('历史页也叠放显示', await main.waitFor(`!!document.querySelector('.detail .thumb-stack')`, 4000));
  await main.click('.nav button[data-page="today"]');
  await sleep(500);
  const files = multi.shots.map((s) => `${D}/${s}`);
  const del = `.item:has(.thumb-stack) .del`;
  await main.eval(`document.querySelector('${del}').style.visibility = 'visible'`);
  await main.click(del);
  await sleep(300);
  await main.click('.app-toast .link');
  await sleep(800);
  check('删了再撤销：3 个图文件都在', files.every((f) => fs.existsSync(f)) && todayItems().length === 2);
  await main.eval(`document.querySelector('${del}').style.visibility = 'visible'`);
  await main.click(del);
  await sleep(6500);
  check('删了等撤销期过去：3 个图文件都删掉', files.every((f) => !fs.existsSync(f)), files.filter((f) => fs.existsSync(f)).length + ' 个还在');
}

if (mode === 'enter-setting') {
  await main.click('.nav button[data-page="settings"]');
  await main.waitFor(`[...document.querySelectorAll('.set-row')].some(r => r.textContent.includes('框选后按回车'))`, 5000);
  const pickEnter = (v) => main.eval(`(() => { const row = [...document.querySelectorAll('.set-row')].find(r => r.textContent.includes('框选后按回车'));
    const sel = row.querySelector('select'); sel.value = ${JSON.stringify(v)}; sel.dispatchEvent(new Event('change')); return sel.value; })()`);
  check('设置页有"框选后按回车"，默认是复制', (await main.eval(`[...document.querySelectorAll('.set-row')].find(r => r.textContent.includes('框选后按回车')).querySelector('select').value`)) === 'copy');
  await pickEnter('record');
  await sleep(500);
  check('改成记录后存进设置', settings().enterAction === 'record', String(settings().enterAction));
  let { ov, info } = await capture('hotkey');
  check('改成记录后：热键截图主按钮是记录', info?.primary === 'record', JSON.stringify(info?.primary));
  await inv(ov, 'capture_cancel');
  await sleep(500);
  await pickEnter('copy');
  await sleep(500);
  ({ ov, info } = await capture('hotkey'));
  check('改回复制后：主按钮是复制', info?.primary === 'copy', JSON.stringify(info?.primary));
  await inv(ov, 'capture_cancel');
  ov.close();
}

if (mode === 'copy-text') {
  const ROOT = decodeURIComponent(new URL('../..', import.meta.url).pathname).replace(/^\/([A-Za-z]:)/, '$1');
  const read = (f) => fs.readFileSync(`${ROOT}/${f}`, 'utf8');
  for (const f of ['src/js/guide.js', 'docs/首次安装说明.md', 'README.md', 'src/js/onboarding.js']) {
    const t = read(f);
    check(`${f} 讲了 Tab 和复制`, t.includes('Tab') && t.includes('复制'));
  }
  const walk = (dir) => fs.readdirSync(`${ROOT}/${dir}`, { withFileTypes: true })
    .flatMap((e) => (e.isDirectory() ? walk(`${dir}/${e.name}`) : [`${dir}/${e.name}`]));
  const files = [...walk('src'), 'docs/首次安装说明.md', 'README.md'].filter((f) => /\.(js|html|css|md)$/.test(f));
  const hits = files.filter((f) => /比如|例如|如：/.test(read(f)));
  check('界面和说明里没有"比如/例如/如："', hits.length === 0, hits.join(', '));
  check('引导和说明一致：两处都有"再截一张"', read('src/js/guide.js').includes('再截一张') && read('docs/首次安装说明.md').includes('再截一张'));
}

if (mode === 'text-outside') {
  const { ov } = await capture('hotkey');
  await drag(ov, [200, 200], [600, 500]);
  await pickTool(ov, 'text');
  await mouseAt(ov, 260, 300);
  await ov.waitFor(`document.activeElement?.id === 'textin'`, 2000);
  await ov.type('字');
  await mouseAt(ov, 900, 700);
  await sleep(300);
  check('打字时点框外：字留下了', await shapeCount(ov) === 1, String(await shapeCount(ov)));
  check('打字时点框外：选框还在、没变', await shown(ov, '#sel') && (await ov.eval(`document.querySelector('#size').textContent`)) === '600 × 450',
    await ov.eval(`document.querySelector('#size').textContent`));
  await ov.key('Escape');
  ov.close();
}

if (mode === 'import-during-capture') {
  const at = makeImages({ 'k1.png': [40, 30], 'k2.png': [50, 30] });
  // 没有草稿时：截图开着，加图完成 → 不能收起框选窗口
  let { ov } = await capture('hotkey');
  await inv(main, 'add_image_paths', { paths: [at('k1.png')] });
  await sleep(500);
  check('截图途中加图：框选窗口还开着', await visible(ov));
  check('截图途中加图：图进了草稿', draft()?.shots?.length === 1, JSON.stringify(draft()));
  await ov.key('Escape');
  await sleep(700);
  const note = await connect('note.html');
  check('截图结束后补一句小窗出来，带着加的图', await visible(note) && (await inv(note, 'note_current'))?.shots?.length === 1);
  // 有草稿时：再截一张途中加图完成 → 框选窗口还开着，之后还能正常截
  await inv(note, 'note_more');
  await sleep(400);
  ({ ov } = await capture('hotkey'));
  await inv(main, 'add_image_paths', { paths: [at('k2.png')] });
  await sleep(500);
  check('再截一张途中加图：框选窗口还开着', await visible(ov));
  await inv(ov, 'capture_done', { index: 0, x: 10, y: 10, w: 80, h: 60, action: 'add' });
  await sleep(700);
  check('之后照常加进这条：3 张', draft()?.shots?.length === 3, JSON.stringify(draft()?.shots));
  ({ ov } = await capture('hotkey'));
  check('热键没有卡住', await visible(ov));
  await ov.key('Escape');
  note.close();
  ov.close();
}

if (mode === 'decode-fail') {
  const { ov } = await capture('main');
  await drag(ov, [200, 200], [600, 500]);
  await pickTool(ov, 'mosaic');
  await drag(ov, [250, 250], [400, 350]);
  // 模拟定格画面读不出来
  await ov.eval(`new Promise((r) => { const bg = document.querySelector('#bg'); bg.onerror = () => r(); bg.src = 'http://frozen.localhost/999?w=1'; })`);
  await ov.key('Tab');
  await sleep(1500);
  check('画面读不出时：不把没打码的原图记下来', draft() === null && shotFiles().length === 0, JSON.stringify(draft()));
  const h = await ov.eval(`document.querySelector('#hint').textContent`);
  check('提示行说明了原因', h.includes('没加载出来'), h);
  check('框选画面还在，可以 Esc 重来', await visible(ov));
  await ov.key('Escape');
  ov.close();
}

if (mode === 'full-nine') {
  const at = makeImages({ 'f.png': [30, 20] });
  const note = await recordOne();
  await inv(main, 'add_image_paths', { paths: Array(8).fill(at('f.png')) });
  await sleep(800);
  await inv(note, 'note_more');
  await sleep(400);
  const { ov, info } = await capture('hotkey');
  check('满 9 张：主按钮是复制，标明满了', info?.primary === 'copy' && info?.full === true, JSON.stringify(info));
  await drag(ov, [200, 200], [500, 400]);
  const acts = await ov.eval(`[...document.querySelectorAll('#toolbar .act')].map(b => b.dataset.act + (b.classList.contains('primary') ? '*' : '')).join(',')`);
  check('满 9 张：工具栏只有保存和复制', acts === 'save,copy*', acts);
  const h = await ov.eval(`document.querySelector('#hint').textContent`);
  check('满 9 张：提示行说了', h.includes('已满 9 张'), h);
  await ov.key('Tab');
  await sleep(500);
  check('满 9 张：Tab 不加图', draft()?.shots?.length === 9 && await visible(ov));
  await ov.key('Escape');
  note.close();
  ov.close();
}

if (mode === 'note-typing-race') {
  const at = makeImages({ 'r.png': [30, 20] });
  const note = await recordOne();
  await note.eval(`document.querySelector('#text').focus()`);
  await note.type('正在打的字');
  // 防抖 300ms 还没存进草稿，加图就完成了
  await inv(main, 'add_image_paths', { paths: [at('r.png')] });
  await sleep(900);
  check('加图完成后输入框里的字还在', (await note.eval(`document.querySelector('#text').value`)) === '正在打的字', await note.eval(`document.querySelector('#text').value`));
  check('字也存进了草稿', draft()?.note === '正在打的字', JSON.stringify(draft()?.note));
  note.close();
}

if (mode === 'second-screen') {
  const note = await recordOne();
  const first = draft().monitor;
  await inv(note, 'note_more');
  await sleep(400);
  const { ov } = await capture('hotkey');
  await inv(ov, 'capture_done', { index: 1, x: 10, y: 10, w: 100, h: 80, action: 'add' });
  await sleep(700);
  const d = draft();
  check('第二张截在另一块屏：草稿记的屏跟着换', d?.shots?.length === 2 && JSON.stringify(d.monitor) !== JSON.stringify(first), `${JSON.stringify(first)} → ${JSON.stringify(d?.monitor)}`);
  note.close();
  ov.close();
}

if (mode === 'focus-last-rect') {
  const { ov: ov1 } = await capture('main');
  await inv(ov1, 'capture_done', { index: 1, x: 50, y: 50, w: 200, h: 150, action: 'record' });
  await sleep(600);
  const note = await connect('note.html');
  await inv(note, 'note_more');
  await sleep(400);
  await capture('hotkey');
  await sleep(500);
  const ov = await connect('capture.html?i=1');
  const info = await inv(ov, 'overlay_init', { index: 1 });
  check('上次的框在第 2 块屏', !!info?.lastRect, JSON.stringify(info));
  const ov0 = await connect('capture.html?i=0');
  const fg = execSync(`python -I -c "import ctypes, sys; sys.stdout.reconfigure(encoding='utf-8'); u=ctypes.windll.user32; b=ctypes.create_unicode_buffer(256); u.GetWindowTextW(u.GetForegroundWindow(), b, 256); print(b.value)"`, { encoding: 'utf8' }).trim();
  check('焦点给了摆着上次那个框的屏，回车直接能用', await ov.eval('document.hasFocus()'), JSON.stringify({ screen1: await ov.eval('document.hasFocus()'), screen0: await ov0.eval('document.hasFocus()'), foreground: fg }));
  ov0.close();
  await ov.key('Escape');
  note.close();
  ov.close();
}

if (mode === 'text-color') {
  const { ov } = await capture('hotkey');
  await drag(ov, [200, 200], [600, 500]);
  await pickTool(ov, 'text');
  await mouseAt(ov, 260, 300);
  await ov.waitFor(`document.activeElement?.id === 'textin'`, 2000);
  await ov.type('字');
  await ov.click('#style [data-color="#ffb900"]');
  check('打字时点颜色：还在打字', (await ov.eval(`document.activeElement?.id`)) === 'textin' && await shapeCount(ov) === 0,
    await ov.eval(`JSON.stringify({ active: document.activeElement?.id || document.activeElement?.tagName, shapes: window.__shapes.length, style: !document.querySelector('#style').hidden, dot: !!document.querySelector('#style [data-color="#ffb900"]'), textin: document.querySelector('#textin').style.display, ls: localStorage.getItem('dwl-annotate-style') })`));
  await ov.type('符');
  await ov.key('Enter');
  await sleep(200);
  const s = await ov.eval(`JSON.stringify(window.__shapes[0] ?? null)`);
  check('一整段字、用了新颜色', s.includes('"text":"字符"') && s.includes('#ffb900'), s);
  await ov.key('Escape');
  ov.close();
}

if (mode === 'bar-width') {
  await inv(main, 'set_hotkey', { accel: 'Ctrl+Shift+Alt+F12' });
  const note = await recordOne();
  await inv(note, 'note_more');
  await sleep(500);
  const bar = await connect('bar.html');
  const r = await bar.eval(`JSON.stringify({ right: document.querySelector('#done').getBoundingClientRect().right, w: innerWidth })`);
  const { right, w } = JSON.parse(r);
  check('四键热键时小条按钮也完整显示', right <= w - 2, r);
  bar.close();
  note.close();
}

if (mode === 'paste-bad') {
  const msg = await main.eval(`window.__TAURI__.core.invoke('add_image_bytes', new Uint8Array([1, 2, 3, 4]))`);
  check('粘贴读不出的图：有提示', typeof msg === 'string' && msg.includes('读不出来'), JSON.stringify(msg));
  check('粘贴读不出的图：不开小窗、没有草稿', draft() === null);
}

if (mode === 'guide-follows-setting') {
  const fabTitle = await main.eval(`document.querySelector('.fab').title`);
  check('截一张按钮的提示说清是记录', !fabTitle.includes('与快捷键相同') && fabTitle.includes('记'), fabTitle);
  await inv(main, 'save_settings', { patch: { enterAction: 'record' } });
  await main.click('.nav button[data-page="settings"]');
  await main.waitFor(`[...document.querySelectorAll('.set-row')].some(r => r.textContent.includes('首次安装说明'))`, 5000);
  await main.eval(`[...document.querySelectorAll('.set-row')].find(r => r.textContent.includes('首次安装说明')).querySelector('button').click()`);
  await sleep(300);
  const t = await main.eval(`document.querySelector('.guide')?.textContent ?? ''`);
  check('设成回车记录后，说明里写的是回车记录', t.includes('按回车记') && !t.includes('按回车复制'), t.slice(t.indexOf('平时怎么用'), t.indexOf('平时怎么用') + 120));
}

if (mode === 'stray-click') {
  const { ov } = await capture('hotkey');
  await drag(ov, [200, 200], [600, 500]);
  const before = await ov.eval(`document.querySelector('#size').textContent`);
  await mouseAt(ov, 900, 700);
  await sleep(200);
  check('框外点一下（没拖）：选框还在', await shown(ov, '#sel') && await shown(ov, '#toolbar') && (await ov.eval(`document.querySelector('#size').textContent`)) === before,
    await ov.eval(`document.querySelector('#size').textContent`));
  await drag(ov, [700, 600], [900, 800]);
  check('框外拖一下：照常重新框选', (await ov.eval(`document.querySelector('#size').textContent`)) === '300 × 300', await ov.eval(`document.querySelector('#size').textContent`));
  await ov.key('Escape');
  ov.close();
}

main.close();
console.log(failures() ? `${failures()} 项失败` : '全部通过');
process.exit(failures() ? 1 : 0);
