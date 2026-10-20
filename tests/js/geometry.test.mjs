import { test } from 'node:test';
import assert from 'node:assert/strict';
import { toPhys, fromPhys, normalize, hitHandle, resizeRect, moveRect, toolbarPos } from '../../src/js/capture/geometry.js';

const view = { w: 1920, h: 1080 };
const tb = { w: 360, h: 40 };

test('toPhys_scale_1_5', () => {
  assert.deepEqual(toPhys({ x: 10.4, y: 20.2, w: 100.3, h: 50.1 }, 1.5), { x: 16, y: 30, w: 150, h: 75 });
});

test('fromPhys_inverts_scale', () => {
  assert.deepEqual(fromPhys({ x: 150, y: 30, w: 300, h: 75 }, 1.5), { x: 100, y: 20, w: 200, h: 50 });
});

test('normalize_any_drag_direction', () => {
  assert.deepEqual(normalize({ x: 300, y: 200 }, { x: 100, y: 250 }), { x: 100, y: 200, w: 200, h: 50 });
});

test('toolbar_below_when_space', () => {
  assert.deepEqual(toolbarPos({ x: 100, y: 100, w: 400, h: 200 }, tb, view), { x: 100, y: 308, place: 'below' });
});

test('toolbar_above_when_no_room_below', () => {
  assert.deepEqual(toolbarPos({ x: 100, y: 900, w: 400, h: 170 }, tb, view), { x: 100, y: 852, place: 'above' });
});

test('toolbar_inside_when_full_screen', () => {
  assert.deepEqual(toolbarPos({ x: 0, y: 20, w: 1920, h: 1050 }, tb, view), { x: 8, y: 1022, place: 'inside' });
});

test('toolbar_x_clamped', () => {
  assert.equal(toolbarPos({ x: 1800, y: 100, w: 100, h: 100 }, tb, view).x, 1552);
});

test('hitHandle_corners_edges_move', () => {
  const r = { x: 100, y: 100, w: 200, h: 100 };
  assert.equal(hitHandle(r, { x: 100, y: 100 }), 'nw');
  assert.equal(hitHandle(r, { x: 303, y: 198 }), 'se');
  assert.equal(hitHandle(r, { x: 200, y: 100 }), 'n');
  assert.equal(hitHandle(r, { x: 300, y: 150 }), 'e');
  assert.equal(hitHandle(r, { x: 200, y: 200 }), 's');
  assert.equal(hitHandle(r, { x: 100, y: 150 }), 'w');
  assert.equal(hitHandle(r, { x: 150, y: 150 }), 'move');
  assert.equal(hitHandle(r, { x: 80, y: 80 }), null);
});

test('resize_flips_past_opposite_edge', () => {
  const r = { x: 100, y: 100, w: 200, h: 100 };
  assert.deepEqual(resizeRect(r, 'nw', 300, 0, view), { x: 300, y: 100, w: 100, h: 100 });
  assert.deepEqual(resizeRect(r, 'se', 10, -5, view), { x: 100, y: 100, w: 210, h: 95 });
  assert.deepEqual(resizeRect(r, 'e', 5000, 0, view), { x: 100, y: 100, w: 1820, h: 100 });
});

test('resize_keeps_minimum_size', () => {
  const r = { x: 100, y: 100, w: 200, h: 100 };
  assert.deepEqual(resizeRect(r, 'e', -200, 0, view), { x: 100, y: 100, w: 4, h: 100 });
});

test('move_clamped_to_view', () => {
  const r = { x: 100, y: 100, w: 200, h: 100 };
  assert.deepEqual(moveRect(r, -500, 2000, view), { x: 0, y: 980, w: 200, h: 100 });
});
