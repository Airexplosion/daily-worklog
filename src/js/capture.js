// @ts-check
// 框选窗口：背景是这块屏按热键时的定格画面，CSS 像素 × devicePixelRatio = 物理像素
// 用 pointer 事件：坐标带小数，150% 缩放下换算回物理像素不会差 1 像素

const { invoke } = window.__TAURI__.core;
const current = window.__TAURI__.webviewWindow.getCurrentWebviewWindow();

const index = Number(new URLSearchParams(location.search).get('i'));
const bg = /** @type {HTMLImageElement} */ (document.getElementById('bg'));
const sel = /** @type {HTMLElement} */ (document.getElementById('sel'));
const size = /** @type {HTMLElement} */ (document.getElementById('size'));
const cx = /** @type {HTMLElement} */ (document.getElementById('cx'));
const cy = /** @type {HTMLElement} */ (document.getElementById('cy'));

/** @type {{x: number, y: number} | null} */
let start = null;
/** @type {{x: number, y: number, w: number, h: number} | null} */
let rect = null;
let busy = false;

function reset() {
  start = null;
  rect = null;
  busy = false;
  sel.style.display = 'none';
  size.style.display = 'none';
  document.body.classList.remove('selecting');
}

/** 这块屏的物理宽度（定格画面地址里带的 w），换算比例比 devicePixelRatio 更可靠 */
let physWidth = 0;
const scale = () => (physWidth > 0 ? physWidth / window.innerWidth : window.devicePixelRatio);
/** @param {number} v */
const phys = (v) => Math.round(v * scale());
/** 鼠标可能拖到别的屏上，坐标限制在本屏内 */
const clampX = (/** @type {number} */ v) => Math.min(Math.max(v, 0), window.innerWidth);
const clampY = (/** @type {number} */ v) => Math.min(Math.max(v, 0), window.innerHeight);

function draw() {
  if (!rect) return;
  document.body.classList.add('selecting');
  Object.assign(sel.style, {
    display: 'block',
    left: `${rect.x}px`, top: `${rect.y}px`, width: `${rect.w}px`, height: `${rect.h}px`,
  });
  size.textContent = `${phys(rect.w)} × ${phys(rect.h)}`;
  size.style.display = 'block';
  const above = rect.y >= 26;
  size.style.left = `${rect.x}px`;
  size.style.top = above ? `${rect.y - 24}px` : `${rect.y + 4}px`;
}

/** @param {MouseEvent} e */
function moveCross(e) {
  cx.style.top = `${e.clientY}px`;
  cy.style.left = `${e.clientX}px`;
}

window.addEventListener('pointerdown', (e) => {
  if (e.button !== 0 || busy) return;
  start = { x: clampX(e.clientX), y: clampY(e.clientY) };
  rect = { x: start.x, y: start.y, w: 0, h: 0 };
  /** @type {HTMLElement} */ (document.documentElement).setPointerCapture?.(e.pointerId);
});

window.addEventListener('pointermove', (e) => {
  moveCross(e);
  if (!start) return;
  const cx = clampX(e.clientX);
  const cy = clampY(e.clientY);
  rect = {
    x: Math.min(start.x, cx),
    y: Math.min(start.y, cy),
    w: Math.abs(cx - start.x),
    h: Math.abs(cy - start.y),
  };
  draw();
});

window.addEventListener('pointerup', (e) => {
  if (e.button !== 0 || !start || !rect) return;
  start = null;
  if (rect.w < 4 || rect.h < 4) {
    reset();
    return;
  }
  busy = true;
  // 两个角分别换算，避免起点和宽度各自取整后终点差 1 像素
  const x1 = phys(rect.x), y1 = phys(rect.y);
  const x2 = phys(rect.x + rect.w), y2 = phys(rect.y + rect.h);
  invoke('capture_done', { index, x: x1, y: y1, w: x2 - x1, h: y2 - y1, action: 'record' })
    .catch((err) => {
      // 失败时恢复可操作，不能让框选画面卡住
      console.error(err);
      reset();
    });
});

window.addEventListener('contextmenu', (e) => {
  e.preventDefault();
  if (busy) return;
  if (rect) reset();
  else invoke('capture_cancel');
});

window.addEventListener('keydown', (e) => {
  if (e.key === 'Escape') invoke('capture_cancel');
});

/**
 * 先显示暗色遮罩（下面是实时画面，已经可以框选），定格画面加载好后换上。
 * 裁剪始终用按热键那一刻的原图，所以两者之间画面有变化也不影响结果。
 * @param {string} url
 */
function load(url) {
  reset();
  document.body.classList.remove('backdrop');
  physWidth = Number(new URL(url).searchParams.get('w')) || 0;
  bg.classList.remove('ready');
  bg.onload = () => bg.classList.add('ready');
  bg.src = url;
  invoke('overlay_ready', { index });
}

current.listen('capture:start', (/** @type {{payload: {url: string}}} */ ev) => load(ev.payload.url));

// 页面刚建好时可能错过了 capture:start，主动问一次
invoke('overlay_init', { index }).then((info) => { if (info) load(/** @type {{url: string}} */ (info).url); });

// 选完区域后，这块屏的框选窗口变成补充框背后的变暗背景（不再接收鼠标）
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
