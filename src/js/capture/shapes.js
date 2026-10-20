// @ts-check
// 标注图形：样式常量、箭头、马赛克、样式记忆（纯函数），以及在画布上按物理像素画出来

export const COLORS = ['#e81123', '#ffb900', '#16c60c', '#0078d4', '#1b1b1b', '#ffffff'];
/** 线宽，CSS 像素（画时乘缩放） */
export const WIDTHS = [2, 4, 8];
/** 文字工具的三档字号 */
export const FONT_SIZES = [14, 20, 28];
/** 马赛克的三档格子 */
export const MOSAIC_CELLS = [8, 14, 22];
export const TOOLS = /** @type {const} */ (['rect', 'ellipse', 'arrow', 'pen', 'text', 'mosaic']);

/** @typedef {typeof TOOLS[number]} Tool */
/** @typedef {{color: string, size: number}} ToolStyle */
/** @typedef {Record<Tool, ToolStyle>} Style */
/**
 * 坐标是选区内的物理像素
 * @typedef {{tool: Tool, color: string, size: number, x1: number, y1: number, x2: number, y2: number,
 *   points?: [number, number][], text?: string}} Shape
 */

/**
 * 实心三角箭头头部的三个点：尖、两个底角
 * @param {number} x1 @param {number} y1 @param {number} x2 @param {number} y2 @param {number} width
 * @returns {[number, number][]}
 */
export function arrowHead(x1, y1, x2, y2, width) {
  const len = Math.max(10, width * 4);
  const half = Math.max(5, width * 2.5);
  const a = Math.atan2(y2 - y1, x2 - x1);
  const bx = x2 - Math.cos(a) * len;
  const by = y2 - Math.sin(a) * len;
  const nx = -Math.sin(a) * half;
  const ny = Math.cos(a) * half;
  return [[x2, y2], [bx + nx, by + ny], [bx - nx, by - ny]];
}

/**
 * 就地把 rect 内的像素变成 cell×cell 的均色块（真正的像素块，不是模糊，还原不了）；超出图片的部分裁掉
 * @param {Uint8ClampedArray} data RGBA @param {number} imgW @param {number} imgH
 * @param {{x: number, y: number, w: number, h: number}} rect @param {number} cell
 */
export function pixelate(data, imgW, imgH, rect, cell) {
  const c = Math.max(1, Math.round(cell));
  const x0 = Math.max(0, Math.floor(rect.x));
  const y0 = Math.max(0, Math.floor(rect.y));
  const x1 = Math.min(imgW, Math.ceil(rect.x + rect.w));
  const y1 = Math.min(imgH, Math.ceil(rect.y + rect.h));
  for (let by = y0; by < y1; by += c) {
    for (let bx = x0; bx < x1; bx += c) {
      const ex = Math.min(bx + c, x1), ey = Math.min(by + c, y1);
      const sum = [0, 0, 0, 0];
      let n = 0;
      for (let y = by; y < ey; y++) {
        for (let x = bx; x < ex; x++) {
          const i = (y * imgW + x) * 4;
          for (let k = 0; k < 4; k++) sum[k] += data[i + k];
          n++;
        }
      }
      const avg = sum.map((v) => Math.round(v / n));
      for (let y = by; y < ey; y++) {
        for (let x = bx; x < ex; x++) data.set(avg, (y * imgW + x) * 4);
      }
    }
  }
}

const STYLE_KEY = 'dwl-annotate-style';

/** @returns {Style} */
function defaultStyle() {
  return /** @type {Style} */ (Object.fromEntries(TOOLS.map((t) => [t, { color: COLORS[0], size: 1 }])));
}

/**
 * 每个工具记住自己的颜色和粗细；读不到、坏了、存储不能用都回默认（红、第 2 档）
 * @param {{getItem: (k: string) => string | null}} storage @returns {Style}
 */
export function loadStyle(storage) {
  const style = defaultStyle();
  try {
    const saved = JSON.parse(storage.getItem(STYLE_KEY) || '{}');
    for (const t of TOOLS) {
      const s = saved?.[t];
      if (s && COLORS.includes(s.color) && [0, 1, 2].includes(s.size)) style[t] = { color: s.color, size: s.size };
    }
  } catch {
    return defaultStyle();
  }
  return style;
}

/** @param {{setItem: (k: string, v: string) => void}} storage @param {Style} style */
export function saveStyle(storage, style) {
  try {
    storage.setItem(STYLE_KEY, JSON.stringify(style));
  } catch {
    // 存不了就算了，下次用默认
  }
}

const FONT = '"Microsoft YaHei UI", "Microsoft YaHei", sans-serif';
export const fontFor = (/** @type {number} */ px) => `${px}px ${FONT}`;

/**
 * 画一个图形。scale 是 CSS 像素到物理像素的比例（线宽、字号、马赛克格子都按它放大）
 * @param {CanvasRenderingContext2D} ctx @param {Shape} s @param {number} scale
 */
export function drawShape(ctx, s, scale) {
  const lw = WIDTHS[s.size] * scale;
  ctx.save();
  ctx.strokeStyle = s.color;
  ctx.fillStyle = s.color;
  ctx.lineWidth = lw;
  ctx.lineCap = 'round';
  ctx.lineJoin = 'round';
  const x = Math.min(s.x1, s.x2), y = Math.min(s.y1, s.y2);
  const w = Math.abs(s.x2 - s.x1), h = Math.abs(s.y2 - s.y1);
  switch (s.tool) {
    case 'rect':
      ctx.strokeRect(x, y, w, h);
      break;
    case 'ellipse':
      ctx.beginPath();
      ctx.ellipse(x + w / 2, y + h / 2, w / 2, h / 2, 0, 0, Math.PI * 2);
      ctx.stroke();
      break;
    case 'arrow': {
      const head = arrowHead(s.x1, s.y1, s.x2, s.y2, lw);
      const [, a, b] = head;
      ctx.beginPath();
      ctx.moveTo(s.x1, s.y1);
      ctx.lineTo((a[0] + b[0]) / 2, (a[1] + b[1]) / 2);
      ctx.stroke();
      ctx.beginPath();
      ctx.moveTo(...head[0]);
      ctx.lineTo(...a);
      ctx.lineTo(...b);
      ctx.closePath();
      ctx.fill();
      break;
    }
    case 'pen': {
      const pts = s.points ?? [];
      if (!pts.length) break;
      ctx.beginPath();
      ctx.moveTo(...pts[0]);
      for (const p of pts.slice(1)) ctx.lineTo(...p);
      if (pts.length === 1) ctx.lineTo(pts[0][0] + 0.1, pts[0][1]);
      ctx.stroke();
      break;
    }
    case 'text':
      ctx.font = fontFor(FONT_SIZES[s.size] * scale);
      ctx.textBaseline = 'top';
      ctx.fillText(s.text ?? '', s.x1, s.y1);
      break;
    case 'mosaic': {
      const rx = Math.max(0, Math.round(x)), ry = Math.max(0, Math.round(y));
      const rw = Math.min(ctx.canvas.width - rx, Math.round(w)), rh = Math.min(ctx.canvas.height - ry, Math.round(h));
      if (rw <= 0 || rh <= 0) break;
      const img = ctx.getImageData(rx, ry, rw, rh);
      pixelate(img.data, rw, rh, { x: 0, y: 0, w: rw, h: rh }, MOSAIC_CELLS[s.size] * scale);
      ctx.putImageData(img, rx, ry);
      break;
    }
  }
  ctx.restore();
}
