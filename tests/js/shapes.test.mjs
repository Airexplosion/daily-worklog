import { test } from 'node:test';
import assert from 'node:assert/strict';
import { arrowHead, pixelate, loadStyle, saveStyle, COLORS } from '../../src/js/capture/shapes.js';

test('arrow_head_horizontal', () => {
  const [tip, a, b] = arrowHead(0, 0, 100, 0, 4); // 头长 16，半宽 10
  assert.deepEqual(tip, [100, 0]);
  const base = [a, b].map(([x, y]) => [Math.round(x), Math.round(y)]).sort((p, q) => p[1] - q[1]);
  assert.deepEqual(base, [[84, -10], [84, 10]]);
});

test('arrow_head_has_minimum_size', () => {
  const [, a] = arrowHead(0, 0, 0, 100, 1); // 头长至少 10，半宽至少 5
  assert.equal(Math.round(a[1]), 90);
  assert.equal(Math.abs(Math.round(a[0])), 5);
});

/** 4×2 图：每个像素 RGBA 不同 */
function image() {
  const data = new Uint8ClampedArray(4 * 2 * 4);
  for (let i = 0; i < 8; i++) data.set([i * 10, i * 20, 255 - i * 10, 255], i * 4);
  return data;
}
const px = (d, x, y, w = 4) => [...d.slice((y * w + x) * 4, (y * w + x) * 4 + 4)];

test('pixelate_blocks_are_uniform_average', () => {
  const d = image();
  pixelate(d, 4, 2, { x: 0, y: 0, w: 4, h: 2 }, 2);
  // 左块是像素 0、1、4、5：R = (0+10+40+50)/4 = 25
  assert.deepEqual(px(d, 0, 0), [25, 50, 230, 255]);
  assert.deepEqual(px(d, 1, 1), [25, 50, 230, 255]);
  // 右块是像素 2、3、6、7：R = (20+30+60+70)/4 = 45
  assert.deepEqual(px(d, 2, 0), [45, 90, 210, 255]);
  assert.deepEqual(px(d, 3, 1), [45, 90, 210, 255]);
});

test('pixelate_leaves_outside_untouched', () => {
  const d = image();
  const before = px(d, 3, 0);
  pixelate(d, 4, 2, { x: 0, y: 0, w: 2, h: 2 }, 2);
  assert.deepEqual(px(d, 3, 0), before);
  assert.deepEqual(px(d, 2, 1), [60, 120, 195, 255]);
});

test('pixelate_clamps_rect_outside_image', () => {
  const d = image();
  assert.doesNotThrow(() => pixelate(d, 4, 2, { x: -3, y: 1, w: 20, h: 9 }, 3));
  assert.doesNotThrow(() => pixelate(d, 4, 2, { x: 10, y: 10, w: 2, h: 2 }, 2));
});

test('style_roundtrip_and_fallbacks', () => {
  const mem = new Map();
  const storage = { getItem: (k) => mem.get(k) ?? null, setItem: (k, v) => mem.set(k, v) };
  const def = loadStyle(storage);
  assert.deepEqual(def.rect, { color: COLORS[0], size: 1 });
  def.mosaic = { color: COLORS[0], size: 2 };
  saveStyle(storage, def);
  assert.equal(loadStyle(storage).mosaic.size, 2);
  mem.set([...mem.keys()][0], '{broken');
  assert.deepEqual(loadStyle(storage).mosaic, { color: COLORS[0], size: 1 });
  const throwing = { getItem: () => { throw new Error('denied'); }, setItem: () => { throw new Error('denied'); } };
  assert.deepEqual(loadStyle(throwing).text, { color: COLORS[0], size: 1 });
  assert.doesNotThrow(() => saveStyle(throwing, def));
});
