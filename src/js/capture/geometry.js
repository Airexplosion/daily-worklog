// @ts-check
// 框选页的几何计算（纯函数）：CSS 像素和物理像素换算、拖动点、调整与移动选框、工具栏位置

/** @typedef {{x: number, y: number, w: number, h: number}} Rect */
/** @typedef {{x: number, y: number}} Point */
/** @typedef {{w: number, h: number}} Size */
/** @typedef {'nw'|'n'|'ne'|'e'|'se'|'s'|'sw'|'w'|'move'} Handle */

/**
 * 两个角分别换算再相减，避免起点和宽度各自取整后终点差 1 像素
 * @param {Rect} r @param {number} scale @returns {Rect}
 */
export function toPhys(r, scale) {
  const x1 = Math.round(r.x * scale);
  const y1 = Math.round(r.y * scale);
  const x2 = Math.round((r.x + r.w) * scale);
  const y2 = Math.round((r.y + r.h) * scale);
  return { x: x1, y: y1, w: x2 - x1, h: y2 - y1 };
}

/** @param {Rect} r @param {number} scale @returns {Rect} */
export function fromPhys(r, scale) {
  return { x: r.x / scale, y: r.y / scale, w: r.w / scale, h: r.h / scale };
}

/** 两点（任意拖动方向）→ 宽高为正的矩形 @param {Point} a @param {Point} b @returns {Rect} */
export function normalize(a, b) {
  return { x: Math.min(a.x, b.x), y: Math.min(a.y, b.y), w: Math.abs(b.x - a.x), h: Math.abs(b.y - a.y) };
}

/**
 * 鼠标在哪个拖动点上：四角、四边中段、框里（移动），都不是返回 null
 * @param {Rect} r @param {Point} p @param {number} [tol] @returns {Handle | null}
 */
export function hitHandle(r, p, tol = 6) {
  const near = (/** @type {number} */ a, /** @type {number} */ b) => Math.abs(a - b) <= tol;
  const inX = p.x >= r.x - tol && p.x <= r.x + r.w + tol;
  const inY = p.y >= r.y - tol && p.y <= r.y + r.h + tol;
  if (!inX || !inY) return null;
  const left = near(p.x, r.x), right = near(p.x, r.x + r.w);
  const top = near(p.y, r.y), bottom = near(p.y, r.y + r.h);
  if (top && left) return 'nw';
  if (top && right) return 'ne';
  if (bottom && left) return 'sw';
  if (bottom && right) return 'se';
  if (top) return 'n';
  if (bottom) return 's';
  if (left) return 'w';
  if (right) return 'e';
  return 'move';
}

const clamp = (/** @type {number} */ v, /** @type {number} */ lo, /** @type {number} */ hi) => Math.min(Math.max(v, lo), hi);

/**
 * 拖某个拖动点调整大小；越过对边时翻转，结果限制在屏内、不小于 min
 * @param {Rect} r @param {Handle} handle @param {number} dx @param {number} dy @param {Size} view @param {number} [min]
 * @returns {Rect}
 */
export function resizeRect(r, handle, dx, dy, view, min = 4) {
  let x1 = r.x, y1 = r.y, x2 = r.x + r.w, y2 = r.y + r.h;
  if (handle.includes('w')) x1 += dx;
  if (handle.includes('e')) x2 += dx;
  if (handle.includes('n')) y1 += dy;
  if (handle.includes('s')) y2 += dy;
  x1 = clamp(x1, 0, view.w); x2 = clamp(x2, 0, view.w);
  y1 = clamp(y1, 0, view.h); y2 = clamp(y2, 0, view.h);
  const out = normalize({ x: x1, y: y1 }, { x: x2, y: y2 });
  if (out.w < min) {
    if (handle.includes('w') && x1 <= x2) out.x = Math.min(out.x, x2 - min);
    out.w = min;
  }
  if (out.h < min) {
    if (handle.includes('n') && y1 <= y2) out.y = Math.min(out.y, y2 - min);
    out.h = min;
  }
  return out;
}

/** 整个选框平移，不出屏 @param {Rect} r @param {number} dx @param {number} dy @param {Size} view @returns {Rect} */
export function moveRect(r, dx, dy, view) {
  return { x: clamp(r.x + dx, 0, view.w - r.w), y: clamp(r.y + dy, 0, view.h - r.h), w: r.w, h: r.h };
}

/**
 * 工具栏放在选框下方；下面放不下放上方；都放不下放在选框里面靠下。左边和选框对齐，不出屏
 * @param {Rect} sel @param {Size} tb @param {Size} view @param {number} [gap]
 * @returns {{x: number, y: number, place: 'below' | 'above' | 'inside'}}
 */
export function toolbarPos(sel, tb, view, gap = 8) {
  const x = clamp(sel.x, gap, view.w - tb.w - gap);
  const below = sel.y + sel.h + gap;
  if (below + tb.h <= view.h) return { x, y: below, place: 'below' };
  const above = sel.y - gap - tb.h;
  if (above >= 0) return { x, y: above, place: 'above' };
  return { x, y: sel.y + sel.h - gap - tb.h, place: 'inside' };
}
