import { test } from 'node:test';
import assert from 'node:assert/strict';
import { keyAction } from '../../src/js/capture/keys.js';

const k = (key, o = {}) => ({ key, ctrlKey: false, shiftKey: false, isComposing: false, keyCode: 0, ...o });
const sel = { hasRect: true, editingText: false };
const none = { hasRect: false, editingText: false };
const typing = { hasRect: true, editingText: true };

test('enter_is_primary_only_with_rect', () => {
  assert.equal(keyAction(k('Enter'), sel), 'primary');
  assert.equal(keyAction(k('Enter'), none), null);
});

test('shortcuts', () => {
  assert.equal(keyAction(k('Tab'), sel), 'record');
  assert.equal(keyAction(k('c', { ctrlKey: true }), sel), 'copy');
  assert.equal(keyAction(k('C', { ctrlKey: true }), sel), 'copy');
  assert.equal(keyAction(k('s', { ctrlKey: true }), sel), 'save');
  assert.equal(keyAction(k('z', { ctrlKey: true }), sel), 'undo');
  assert.equal(keyAction(k('Escape'), sel), 'cancel');
  assert.equal(keyAction(k('Escape'), none), 'cancel');
  assert.equal(keyAction(k('Tab'), none), null);
  assert.equal(keyAction(k('c', { ctrlKey: true }), none), null);
  assert.equal(keyAction(k('c'), sel), null);
});

test('keys_ignore_ime_composition', () => {
  assert.equal(keyAction(k('Enter', { isComposing: true }), sel), null);
  assert.equal(keyAction(k('Enter', { keyCode: 229 }), sel), null);
  assert.equal(keyAction(k('Escape', { keyCode: 229 }), sel), null);
});

test('keys_text_editing_swallow_enter', () => {
  assert.equal(keyAction(k('Enter'), typing), null);
  assert.equal(keyAction(k('Escape'), typing), null);
  assert.equal(keyAction(k('z', { ctrlKey: true }), typing), null);
  assert.equal(keyAction(k('Tab'), typing), null);
  assert.equal(keyAction(k('c', { ctrlKey: true }), typing), null);
});
