// @ts-check
// 框选窗口：背景是这块屏按热键时的定格画面。松手后选框还能拖边角调整，下方出工具栏；
// 回车（主按钮）、Tab（记录）、Ctrl+C（复制）、Ctrl+S（保存）决定图去哪。
// 用 pointer 事件：坐标带小数，150% 缩放下换算回物理像素不会差 1 像素

import { toPhys, fromPhys, normalize, hitHandle, resizeRect, moveRect, toolbarPos } from './geometry.js';
import { keyAction } from './keys.js';
import { COLORS, FONT_SIZES, drawShape, fontFor, loadStyle, saveStyle } from './shapes.js';

/** @typedef {import('./geometry.js').Rect} Rect */
/** @typedef {import('./geometry.js').Point} Point */
/** @typedef {import('./geometry.js').Handle} Handle */
/** @typedef {'copy' | 'record' | 'add'} Primary */
/** @typedef {{url: string, primary: Primary, shotNo: number, lastRect: Rect | null, hint: string | null, full: boolean}} CaptureInfo */
/** @typedef {import('./shapes.js').Shape} Shape */
/** @typedef {import('./shapes.js').Tool} Tool */

const { invoke } = window.__TAURI__.core;
const current = window.__TAURI__.webviewWindow.getCurrentWebviewWindow();

const index = Number(new URLSearchParams(location.search).get('i'));
const $ = (/** @type {string} */ id) => /** @type {HTMLElement} */ (document.getElementById(id));
const bg = /** @type {HTMLImageElement} */ ($('bg'));
const sel = $('sel');
const size = $('size');
const cx = $('cx');
const cy = $('cy');
const hint = $('hint');
const toolbar = $('toolbar');
const styleRow = $('style');
const paint = /** @type {HTMLCanvasElement} */ ($('paint'));
const textin = /** @type {HTMLInputElement} */ ($('textin'));
// 读定格画面的像素（马赛克、合成）需要跨源许可，必须在设置 src 之前
bg.crossOrigin = 'anonymous';

/** 本机存储不能用时（隐私模式等）也照常工作，只是不记颜色粗细 */
const storage = (() => {
  try {
    return window.localStorage;
  } catch {
    return { getItem: () => null, setItem: () => {} };
  }
})();
let style = loadStyle(storage);

const TOOL_ICONS = {
  rect: '<rect x="3.5" y="5.5" width="13" height="9" rx="1"/>',
  ellipse: '<ellipse cx="10" cy="10" rx="7" ry="5"/>',
  arrow: '<path d="M4 16 15 5M8 5h7v7"/>',
  pen: '<path d="M4 16c3-1 4-6 7-7s3 3 5 1"/>',
  text: '<path d="M5 5h10M10 5v11"/>',
  mosaic: '<path d="M4 4h4v4H4zM12 4h4v4h-4zM8 8h4v4H8zM4 12h4v4H4zM12 12h4v4h-4z"/>',
};
const TOOL_NAMES = { rect: '矩形', ellipse: '椭圆（按住 Shift 是正圆）', arrow: '箭头', pen: '画笔', text: '文字', mosaic: '马赛克' };
const icon = (/** @type {string} */ body) =>
  `<svg viewBox="0 0 20 20" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round">${body}</svg>`;

/** @type {CaptureInfo | null} */
let info = null;
/** CSS 像素 @type {Rect | null} */
let rect = null;
/** @type {null | {kind: 'new', start: Point} | {kind: 'handle', handle: Handle, start: Point, orig: Rect}} */
let drag = null;
let busy = false;
/** 画了标注以后选框固定 */
let locked = false;
/** 当前标注工具 @type {Tool | null} */
let tool = null;
/** 已画的标注（选区内物理像素） @type {Shape[]} */
let shapes = [];
/** 正在画的那一笔 @type {Shape | null} */
let drawing = null;
/** 文字工具：正在打字的位置（选区内物理像素） @type {{x: number, y: number} | null} */
let textAt = null;
let bgReady = false;
/** 这次截图的升级提示已经显示过 */
let hintSeen = false;
// 测试只读
Object.defineProperty(window, '__shapes', { get: () => shapes.slice() });

/** 这块屏的物理宽度（定格画面地址里带的 w），换算比例比 devicePixelRatio 更可靠 */
let physWidth = 0;
const scale = () => (physWidth > 0 ? physWidth / window.innerWidth : window.devicePixelRatio);
const view = () => ({ w: window.innerWidth, h: window.innerHeight });
/** 鼠标可能拖到别的屏上，坐标限制在本屏内 */
const clampPoint = (/** @type {PointerEvent | MouseEvent} */ e) => ({
  x: Math.min(Math.max(e.clientX, 0), window.innerWidth),
  y: Math.min(Math.max(e.clientY, 0), window.innerHeight),
});

function setHint() {
  hint.className = '';
  if (!rect) {
    hint.innerHTML = '拖动鼠标框选　<b>Esc</b> 取消　<b>右键</b> 重选';
  } else if (info?.hint) {
    hint.className = 'warn';
    hint.textContent = info.hint;
    // 用户真的看到了才算数（框选前就按 Esc 的不算）
    if (!hintSeen) {
      hintSeen = true;
      invoke('hint_seen');
    }
  } else if (info?.full) {
    hint.innerHTML = '这条已满 9 张，只能复制或保存　<b>Enter</b> 复制　<b>Ctrl+S</b> 保存　<b>Esc</b> 取消';
  } else if (info?.primary === 'add') {
    hint.innerHTML = `已截 ${info.shotNo - 1} 张 · <b>回车</b> 沿用上次的框，或重新拖一个`;
  } else if (info?.primary === 'record') {
    hint.innerHTML = '<b>Enter</b> 记录　<b>Ctrl+C</b> 复制　<b>Ctrl+S</b> 保存　<b>Esc</b> 取消';
  } else {
    hint.innerHTML = '<b>Enter</b> 复制　<b>Tab</b> 记录　<b>Ctrl+S</b> 保存　<b>Esc</b> 取消';
  }
}

/** 工具栏：左边标注工具和撤销，右边三个去向按钮（主按钮蓝色、放最后），最后是取消 */
function buildToolbar() {
  const primary = info?.primary ?? 'copy';
  /** @type {[string, string, string][]} */
  const acts = info?.full
    // 没记完的那条已满 9 张：加不进去了，只能复制或另存
    ? [['save', '保存…', 'Ctrl+S'], ['copy', '复制', 'Enter']]
    : primary === 'copy'
    ? [['save', '保存…', 'Ctrl+S'], ['record', '记录', 'Tab'], ['copy', '复制', 'Enter']]
    : [['save', '保存…', 'Ctrl+S'], ['copy', '复制', 'Ctrl+C'],
      primary === 'add' ? ['add', `加到这条（第 ${info?.shotNo ?? 2} 张）`, 'Enter'] : ['record', '记录', 'Enter']];
  // 定格画面还没加载好时也能选工具、能画：画的先记着，画面一到就显示；导出时会等画面
  const tools = Object.entries(TOOL_ICONS)
    .map(([k, body]) => `<button class="i" data-tool="${k}" title="${TOOL_NAMES[/** @type {keyof TOOL_NAMES} */ (k)]}">${icon(body)}</button>`)
    .join('');
  const undo = `<button class="i" data-act="undo" title="撤销（Ctrl+Z）">${icon('<path d="M7 5 3.5 8.5 7 12M4 8.5h8a4 4 0 0 1 0 8H9"/>')}</button>`;
  const buttons = acts
    .map(([act, label, key], i) => `<button class="act${i === acts.length - 1 ? ' primary' : ''}" data-act="${act}">${label}<kbd>${key}</kbd></button>`)
    .join('');
  const close = `<button class="i" data-act="cancel" title="取消（Esc）">${icon('<path d="M5 5l10 10M15 5 5 15"/>')}</button>`;
  toolbar.innerHTML = `${tools}<span class="sep"></span>${undo}<span class="sep"></span>${buttons}${close}`;
}

function draw() {
  if (!rect) return;
  document.body.classList.add('selecting');
  Object.assign(sel.style, {
    display: 'block',
    left: `${rect.x}px`, top: `${rect.y}px`, width: `${rect.w}px`, height: `${rect.h}px`,
  });
  const p = toPhys(rect, scale());
  size.textContent = `${p.w} × ${p.h}`;
  size.style.display = 'block';
  const above = rect.y >= 26;
  size.style.left = `${rect.x}px`;
  size.style.top = above ? `${rect.y - 24}px` : `${rect.y + 4}px`;
  render();
}

/** 选区内的物理像素坐标 @param {Point} p @returns {[number, number]} */
function rel(p) {
  if (!rect) return [0, 0];
  const s = scale();
  const r = toPhys(rect, s);
  return [Math.min(Math.max(p.x * s - r.x, 0), r.w), Math.min(Math.max(p.y * s - r.y, 0), r.h)];
}

/** 画布盖在选框上：先画定格画面这一块，再按顺序画标注。没有标注时不显示（背景本来就一样） */
function render() {
  if (!rect || !bgReady || (!shapes.length && !drawing)) {
    paint.style.display = 'none';
    return;
  }
  const s = scale();
  const r = toPhys(rect, s);
  if (paint.width !== r.w) paint.width = r.w;
  if (paint.height !== r.h) paint.height = r.h;
  Object.assign(paint.style, { display: 'block', left: `${rect.x}px`, top: `${rect.y}px`, width: `${rect.w}px`, height: `${rect.h}px` });
  const ctx = /** @type {CanvasRenderingContext2D} */ (paint.getContext('2d', { willReadFrequently: true }));
  ctx.drawImage(bg, r.x, r.y, r.w, r.h, 0, 0, r.w, r.h);
  for (const sh of shapes) drawShape(ctx, sh, s);
  if (drawing) drawShape(ctx, drawing, s);
}

function placeToolbar() {
  if (!rect) return;
  toolbar.hidden = false;
  const pos = toolbarPos(rect, { w: toolbar.offsetWidth, h: toolbar.offsetHeight }, view());
  toolbar.style.left = `${pos.x}px`;
  toolbar.style.top = `${pos.y}px`;
  placeStyleRow(pos.place === 'above');
}

/** 颜色、粗细这一行：跟着工具栏，工具栏在选框上方时它在工具栏上面 @param {boolean} [up] */
function placeStyleRow(up = toolbar.offsetTop < (rect?.y ?? 0)) {
  if (!tool || toolbar.hidden) {
    styleRow.hidden = true;
    return;
  }
  const st = style[tool];
  const titles = tool === 'mosaic' ? ['小格', '中格', '大格'] : tool === 'text' ? ['小字', '中字', '大字'] : ['细', '中', '粗'];
  const colors = tool === 'mosaic' ? ''
    : COLORS.map((c) => `<button class="dot${c === st.color ? ' on' : ''}" data-color="${c}" style="background:${c}" title="颜色"></button>`).join('') + '<span class="sep"></span>';
  const sizes = [0, 1, 2]
    .map((i) => `<button class="sz${i === st.size ? ' on' : ''}" data-size="${i}" title="${titles[i]}"><span style="width:${[4, 7, 11][i]}px;height:${[4, 7, 11][i]}px"></span></button>`)
    .join('');
  styleRow.innerHTML = colors + sizes;
  styleRow.hidden = false;
  styleRow.style.left = `${toolbar.offsetLeft}px`;
  styleRow.style.top = up ? `${toolbar.offsetTop - styleRow.offsetHeight - 6}px` : `${toolbar.offsetTop + toolbar.offsetHeight + 6}px`;
}

/** @param {Tool | null} next */
function pickTool(next) {
  commitText();
  tool = next;
  toolbar.querySelectorAll('[data-tool]').forEach((b) => b.classList.toggle('on', /** @type {HTMLElement} */ (b).dataset.tool === tool));
  document.body.classList.toggle('tool', !!tool);
  placeStyleRow();
}

/** 画了第一笔以后选框固定；全部撤销后又能调 */
function setLocked(on) {
  locked = on;
  document.body.classList.toggle('locked', on);
}

/** 文字工具：在点的地方就地打字 @param {Point} p */
function startText(p) {
  if (!tool) return;
  const [x, y] = rel(p);
  textAt = { x, y };
  const st = style[tool];
  Object.assign(textin.style, { display: 'block', left: `${p.x}px`, top: `${p.y}px`, color: st.color, font: fontFor(FONT_SIZES[st.size]) });
  textin.value = '';
  setTimeout(() => textin.focus());
}

function commitText() {
  if (!textAt) return;
  const text = textin.value.trim();
  if (text) {
    const st = style.text;
    shapes.push({ tool: 'text', color: st.color, size: st.size, x1: textAt.x, y1: textAt.y, x2: textAt.x, y2: textAt.y, text });
    setLocked(true);
  }
  textAt = null;
  textin.style.display = 'none';
  textin.blur();
  render();
}

function undo() {
  if (textAt) {
    textAt = null;
    textin.style.display = 'none';
    return;
  }
  shapes.pop();
  if (!shapes.length) setLocked(false);
  render();
}

/** 选好了：出拖动点和工具栏 */
function selected() {
  document.body.classList.add('selected');
  draw();
  placeToolbar();
  setHint();
}

function reset() {
  rect = null;
  drag = null;
  busy = false;
  locked = false;
  shapes = [];
  drawing = null;
  textAt = null;
  tool = null;
  sel.style.display = 'none';
  size.style.display = 'none';
  paint.style.display = 'none';
  textin.style.display = 'none';
  toolbar.hidden = true;
  styleRow.hidden = true;
  toolbar.querySelectorAll('[data-tool]').forEach((b) => b.classList.remove('on'));
  document.body.classList.remove('selecting', 'selected', 'dragging', 'locked', 'tool');
  setHint();
}

/** @param {MouseEvent} e */
function moveCross(e) {
  cx.style.top = `${e.clientY}px`;
  cy.style.left = `${e.clientX}px`;
}

/** @param {string} name */
async function act(name) {
  if (busy) return;
  if (name === 'cancel') {
    invoke('capture_cancel');
    return;
  }
  if (name === 'undo') {
    undo();
    return;
  }
  if (!rect) return;
  if (info?.full && (name === 'record' || name === 'add')) return;
  commitText();
  const action = name === 'primary' ? info?.primary ?? 'copy' : name;
  const r = toPhys(rect, scale());
  busy = true;
  try {
    if (shapes.length) {
      // 定格画面读不出来（极少见）就合成不了标注。不能退回不带标注的原图：打了码的地方会原样泄露出去
      const base = await bg.decode().then(() => true, () => false);
      if (!base) {
        hint.className = 'warn';
        hint.textContent = '画面没加载出来，标注合成不了；按 Esc 重新截';
        busy = false;
        return;
      }
      // 有标注：在画布上按物理像素合成，把 RGBA 原始字节交给后端
      bgReady = true;
      render();
      const ctx = /** @type {CanvasRenderingContext2D} */ (paint.getContext('2d', { willReadFrequently: true }));
      const pixels = ctx.getImageData(0, 0, r.w, r.h).data;
      await invoke('capture_pixels', new Uint8Array(pixels.buffer), {
        headers: {
          'x-index': String(index), 'x-x': String(r.x), 'x-y': String(r.y),
          'x-width': String(r.w), 'x-height': String(r.h), 'x-action': action,
        },
      });
    } else {
      // 没有标注：后端从定格原图裁剪，最快也最准
      await invoke('capture_done', { index, ...r, action });
    }
  } catch (err) {
    // 失败时恢复可操作，不能让框选画面卡住
    console.error(err);
    busy = false;
  }
}

for (const el of [toolbar, styleRow, textin]) {
  el.addEventListener('pointerdown', (e) => e.stopPropagation());
  el.addEventListener('dblclick', (e) => e.stopPropagation());
}
toolbar.addEventListener('click', (e) => {
  const target = /** @type {HTMLElement} */ (e.target);
  const btn = /** @type {HTMLButtonElement | null} */ (target.closest('[data-act], [data-tool]'));
  if (!btn || btn.disabled) return;
  if (btn.dataset.tool) {
    const t = /** @type {Tool} */ (btn.dataset.tool);
    pickTool(tool === t ? null : t);
  } else if (btn.dataset.act) {
    act(btn.dataset.act);
  }
});
styleRow.addEventListener('click', (e) => {
  const btn = /** @type {HTMLElement | null} */ (/** @type {HTMLElement} */ (e.target).closest('[data-color], [data-size]'));
  if (!btn || !tool) return;
  if (btn.dataset.color) style[tool].color = btn.dataset.color;
  if (btn.dataset.size) style[tool].size = Number(btn.dataset.size);
  saveStyle(storage, style);
  placeStyleRow();
  if (textAt) {
    Object.assign(textin.style, { color: style.text.color, font: fontFor(FONT_SIZES[style.text.size]) });
    textin.focus();
  }
});

textin.addEventListener('keydown', (e) => {
  // 自己处理，不往上冒：免得结束打字的这下回车又被当成"复制/记录"
  e.stopPropagation();
  if (e.isComposing || e.keyCode === 229) return;
  if (e.key === 'Enter') {
    e.preventDefault();
    commitText();
  } else if (e.key === 'Escape') {
    e.preventDefault();
    textAt = null;
    textin.style.display = 'none';
  }
});
textin.addEventListener('blur', () => commitText());

window.addEventListener('pointerdown', (e) => {
  if (e.button !== 0 || busy) return;
  // 正在打字时点哪儿（框里框外）都只是结束这段文字，不画新的、不重新框选
  if (textAt) {
    commitText();
    return;
  }
  const p = clampPoint(e);
  const handle = rect ? hitHandle(rect, p) : null;
  const inside = !!rect && p.x >= rect.x && p.x <= rect.x + rect.w && p.y >= rect.y && p.y <= rect.y + rect.h;
  if (rect && tool && inside && (locked || handle === 'move')) {
    // 选了工具：在选框里按下就画（选框固定以后，边上也能画）
    if (tool === 'text') {
      startText(p);
      return;
    }
    const st = style[tool];
    const [x, y] = rel(p);
    drawing = { tool, color: st.color, size: st.size, x1: x, y1: y, x2: x, y2: y, points: tool === 'pen' ? [[x, y]] : undefined };
    setLocked(true);
    /** @type {HTMLElement} */ (document.documentElement).setPointerCapture?.(e.pointerId);
    return;
  }
  if (rect && handle && !locked) {
    drag = { kind: 'handle', handle, start: p, orig: rect };
  } else if (rect && handle && locked) {
    return;
  } else if (!locked) {
    drag = { kind: 'new', start: p };
    rect = { x: p.x, y: p.y, w: 0, h: 0 };
    toolbar.hidden = true;
    document.body.classList.remove('selected');
  } else {
    return;
  }
  document.body.classList.add('dragging');
  /** @type {HTMLElement} */ (document.documentElement).setPointerCapture?.(e.pointerId);
});

window.addEventListener('pointermove', (e) => {
  moveCross(e);
  if (drawing) {
    let [x, y] = rel(clampPoint(e));
    if (drawing.tool === 'ellipse' && e.shiftKey) {
      // 按住 Shift 是正圆
      const d = Math.max(Math.abs(x - drawing.x1), Math.abs(y - drawing.y1));
      x = drawing.x1 + Math.sign(x - drawing.x1 || 1) * d;
      y = drawing.y1 + Math.sign(y - drawing.y1 || 1) * d;
    }
    drawing.x2 = x;
    drawing.y2 = y;
    drawing.points?.push([x, y]);
    render();
    return;
  }
  if (!drag) {
    if (rect && !locked) {
      const h = hitHandle(rect, clampPoint(e));
      document.body.style.cursor = h === 'move' ? 'move' : h ? getComputedStyle(/** @type {HTMLElement} */ (sel.querySelector(`[data-h="${h}"]`))).cursor : '';
    }
    return;
  }
  const p = clampPoint(e);
  if (drag.kind === 'new') {
    rect = normalize(drag.start, p);
  } else {
    const dx = p.x - drag.start.x, dy = p.y - drag.start.y;
    rect = drag.handle === 'move' ? moveRect(drag.orig, dx, dy, view()) : resizeRect(drag.orig, drag.handle, dx, dy, view());
  }
  draw();
});

window.addEventListener('pointerup', (e) => {
  if (e.button !== 0) return;
  if (drawing) {
    const d = drawing;
    drawing = null;
    // 点一下没拖开的不算（画笔除外）
    if (d.tool === 'pen' || Math.abs(d.x2 - d.x1) > 2 || Math.abs(d.y2 - d.y1) > 2) shapes.push(d);
    if (!shapes.length) setLocked(false);
    render();
    return;
  }
  if (!drag) return;
  const wasNew = drag.kind === 'new';
  drag = null;
  document.body.classList.remove('dragging');
  if (wasNew && rect && (rect.w < 4 || rect.h < 4)) {
    reset();
    return;
  }
  selected();
});

window.addEventListener('dblclick', (e) => {
  if (!rect || busy || tool) return;
  const p = clampPoint(e);
  if (p.x >= rect.x && p.x <= rect.x + rect.w && p.y >= rect.y && p.y <= rect.y + rect.h) act('primary');
});

window.addEventListener('contextmenu', (e) => {
  e.preventDefault();
  if (busy) return;
  if (rect) reset();
  else invoke('capture_cancel');
});

window.addEventListener('keydown', (e) => {
  const name = keyAction(e, { hasRect: !!rect && !drag && !drawing, editingText: !!textAt });
  if (!name) return;
  e.preventDefault();
  act(name);
});

/**
 * 先显示暗色遮罩（下面是实时画面，已经可以框选），定格画面加载好后换上。
 * 裁剪始终用按热键那一刻的原图，所以两者之间画面有变化也不影响结果。
 * @param {CaptureInfo} next
 */
function load(next) {
  info = next;
  hintSeen = false;
  reset();
  document.body.classList.remove('backdrop');
  physWidth = Number(new URL(next.url).searchParams.get('w')) || 0;
  bgReady = false;
  style = loadStyle(storage);
  buildToolbar();
  bg.classList.remove('ready');
  bg.onload = () => {
    bg.classList.add('ready');
    bgReady = true;
    render();
  };
  bg.src = next.url;
  // 再截一张：上一张的框摆回原位，直接回车就截同一块
  if (next.lastRect) {
    rect = fromPhys(next.lastRect, scale());
    selected();
  }
  invoke('overlay_ready', { index });
}

current.listen('capture:start', (/** @type {{payload: CaptureInfo}} */ ev) => load(ev.payload));

// 页面刚建好时可能错过了 capture:start，主动问一次
invoke('overlay_init', { index }).then((next) => { if (next) load(/** @type {CaptureInfo} */ (next)); });

// 选完区域后，这块屏的框选窗口变成补一句小窗背后的变暗背景（不再接收鼠标）
current.listen('capture:backdrop', () => {
  reset();
  busy = true;
  document.body.classList.add('backdrop');
});

current.listen('capture:reset', () => {
  reset();
  document.body.classList.remove('backdrop');
  bg.classList.remove('ready');
  bg.removeAttribute('src');
});

setHint();
