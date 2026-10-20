// @ts-check
// 框选窗口：背景是这块屏按热键时的定格画面。松手后选框还能拖边角调整，下方出工具栏；
// 回车（主按钮）、Tab（记录）、Ctrl+C（复制）、Ctrl+S（保存）决定图去哪。
// 用 pointer 事件：坐标带小数，150% 缩放下换算回物理像素不会差 1 像素

import { toPhys, fromPhys, normalize, hitHandle, resizeRect, moveRect, toolbarPos } from './geometry.js';
import { keyAction } from './keys.js';

/** @typedef {import('./geometry.js').Rect} Rect */
/** @typedef {import('./geometry.js').Point} Point */
/** @typedef {import('./geometry.js').Handle} Handle */
/** @typedef {'copy' | 'record' | 'add'} Primary */
/** @typedef {{url: string, primary: Primary, shotNo: number, lastRect: Rect | null, hint: string | null}} CaptureInfo */

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
/** 画了标注以后选框固定（Task 6 置位） */
let locked = false;

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
  const acts = primary === 'copy'
    ? [['save', '保存…', 'Ctrl+S'], ['record', '记录', 'Tab'], ['copy', '复制', 'Enter']]
    : [['save', '保存…', 'Ctrl+S'], ['copy', '复制', 'Ctrl+C'],
      primary === 'add' ? ['add', `加到这条（第 ${info?.shotNo ?? 2} 张）`, 'Enter'] : ['record', '记录', 'Enter']];
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
}

function placeToolbar() {
  if (!rect) return;
  toolbar.hidden = false;
  const pos = toolbarPos(rect, { w: toolbar.offsetWidth, h: toolbar.offsetHeight }, view());
  toolbar.style.left = `${pos.x}px`;
  toolbar.style.top = `${pos.y}px`;
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
  sel.style.display = 'none';
  size.style.display = 'none';
  toolbar.hidden = true;
  document.body.classList.remove('selecting', 'selected', 'dragging', 'locked');
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
  if (name === 'undo' || !rect) return;
  const action = name === 'primary' ? info?.primary ?? 'copy' : name;
  busy = true;
  try {
    await invoke('capture_done', { index, ...toPhys(rect, scale()), action });
  } catch (err) {
    // 失败时恢复可操作，不能让框选画面卡住
    console.error(err);
    busy = false;
  }
}

toolbar.addEventListener('pointerdown', (e) => e.stopPropagation());
toolbar.addEventListener('click', (e) => {
  const btn = /** @type {HTMLElement | null} */ (/** @type {HTMLElement} */ (e.target).closest('[data-act]'));
  if (btn?.dataset.act) act(btn.dataset.act);
});

window.addEventListener('pointerdown', (e) => {
  if (e.button !== 0 || busy) return;
  const p = clampPoint(e);
  const handle = rect ? hitHandle(rect, p) : null;
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
  if (e.button !== 0 || !drag) return;
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
  if (!rect || busy) return;
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
  const name = keyAction(e, { hasRect: !!rect && !drag, editingText: false });
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
  reset();
  document.body.classList.remove('backdrop');
  physWidth = Number(new URL(next.url).searchParams.get('w')) || 0;
  buildToolbar();
  bg.classList.remove('ready');
  bg.onload = () => bg.classList.add('ready');
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
