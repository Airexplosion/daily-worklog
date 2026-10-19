// @ts-check
// 设置页和首次引导共用的 Win11 风格控件

import { h, toast, errText } from './ui.js';

const { invoke } = window.__TAURI__.core;

/**
 * 一组设置（白色卡片）
 * @param {string} title
 * @param {...(Node | null | false)} rows
 */
export function section(title, ...rows) {
  return h('section', { class: 'set-sec' }, h('h3', {}, title), h('div', { class: 'set-card' }, ...rows));
}

/**
 * 左边说明、右边控件的一行
 * @param {string} label
 * @param {string | null} desc
 * @param {...(Node | null | false)} controls
 */
export function row(label, desc, ...controls) {
  return h('div', { class: 'set-row' },
    h('div', { class: 'set-label' }, h('div', {}, label), desc ? h('small', {}, desc) : null),
    h('div', { class: 'set-ctl' }, ...controls));
}

/**
 * Win11 开关
 * @param {boolean} on
 * @param {(on: boolean) => void} onChange
 */
export function toggle(on, onChange) {
  const btn = h('button', { class: `switch${on ? ' on' : ''}`, role: 'switch', 'aria-checked': String(on) });
  btn.addEventListener('click', () => {
    const next = !btn.classList.contains('on');
    btn.classList.toggle('on', next);
    btn.setAttribute('aria-checked', String(next));
    onChange(next);
  });
  return btn;
}

/**
 * 输入框：失焦或回车时提交（内容有变化才提交）
 * @param {string} value
 * @param {(v: string) => void} onCommit
 * @param {Record<string, any>} [attrs]
 */
export function field(value, onCommit, attrs = {}) {
  const input = /** @type {HTMLInputElement} */ (h('input', { class: 'input', value, ...attrs }));
  let last = value;
  const commit = () => {
    if (input.value !== last) {
      last = input.value;
      onCommit(input.value);
    }
  };
  input.addEventListener('change', commit);
  input.addEventListener('keydown', (e) => { if (e.key === 'Enter' && !e.isComposing) input.blur(); });
  return input;
}

/**
 * @param {{value: string, label: string}[]} options
 * @param {string} value
 * @param {(v: string) => void} onChange
 */
export function select(options, value, onChange) {
  const el = /** @type {HTMLSelectElement} */ (h('select', { class: 'input' },
    ...options.map((o) => h('option', { value: o.value }, o.label))));
  el.value = value;
  el.addEventListener('change', () => onChange(el.value));
  return el;
}

/**
 * 可删可加的词条（禁用词、标签）
 * @param {string[]} values
 * @param {(values: string[]) => void} onChange
 * @param {{placeholder?: string, max?: number, marks?: Record<string, string>}} [opts]
 */
export function chips(values, onChange, opts = {}) {
  let list = [...values];
  const box = h('div', { class: 'chips' });
  const render = () => {
    box.innerHTML = '';
    for (const v of list) {
      const mark = opts.marks?.[v];
      box.append(h('span', { class: 'chip' }, v, mark ? h('span', { class: 'chip-mark' }, mark) : null,
        h('button', { class: 'chip-x', title: '删掉', onclick: () => { list = list.filter((x) => x !== v); render(); onChange(list); } }, '✕')));
    }
    const input = /** @type {HTMLInputElement} */ (h('input', { class: 'chip-input', placeholder: opts.placeholder || '＋ 添加', maxlength: opts.max || 12 }));
    input.addEventListener('keydown', (e) => {
      if (e.isComposing || e.keyCode === 229 || e.key !== 'Enter') return;
      const v = input.value.trim();
      if (v && !list.includes(v)) {
        list.push(v);
        render();
        onChange(list);
        /** @type {HTMLInputElement} */ (box.querySelector('.chip-input')).focus();
      }
    });
    box.append(input);
  };
  render();
  return Object.assign(box, { setValues: (/** @type {string[]} */ v) => { list = [...v]; render(); } });
}

const KEY_NAMES = /** @type {Record<string, string>} */ ({ Space: 'Space', Backquote: '`', Minus: '-', Equal: '=' });

/** @param {KeyboardEvent} e @returns {string | null} */
function keyName(e) {
  if (/^Key[A-Z]$/.test(e.code)) return e.code.slice(3);
  if (/^Digit\d$/.test(e.code)) return e.code.slice(5);
  if (/^F([1-9]|1[0-2])$/.test(e.code)) return e.code;
  return KEY_NAMES[e.code] ?? null;
}

/**
 * 快捷键录制：点一下，再按新组合；当场注册，被占用就提示
 * @param {string} current
 * @param {(accel: string) => void} [onSaved]
 */
export function hotkeyRecorder(current, onSaved) {
  const btn = h('button', { class: 'hotkey' });
  const status = h('span', { class: 'hk-status' });
  const show = (/** @type {string} */ accel) => {
    btn.innerHTML = '';
    btn.append(...accel.split('+').map((k) => h('kbd', {}, k)));
  };
  show(current);
  let recording = false;
  const onKey = async (/** @type {KeyboardEvent} */ e) => {
    e.preventDefault();
    e.stopPropagation();
    if (e.key === 'Escape') return stop(current);
    const key = keyName(e);
    if (!key) return;
    if (!e.ctrlKey && !e.altKey) {
      status.className = 'hk-status warn';
      status.textContent = '要带上 Ctrl 或 Alt';
      return;
    }
    const accel = [e.ctrlKey && 'Ctrl', e.altKey && 'Alt', e.shiftKey && 'Shift', key].filter(Boolean).join('+');
    try {
      await invoke('set_hotkey', { accel });
      current = accel;
      status.className = 'hk-status ok';
      status.textContent = '✓ 可用';
      onSaved?.(accel);
    } catch (err) {
      status.className = 'hk-status warn';
      status.textContent = errText(err);
    }
    stop(current);
  };
  const stop = (/** @type {string} */ accel) => {
    recording = false;
    btn.classList.remove('rec');
    window.removeEventListener('keydown', onKey, true);
    show(accel);
  };
  btn.addEventListener('click', () => {
    if (recording) return stop(current);
    recording = true;
    btn.classList.add('rec');
    btn.textContent = '按下新的组合键…（Esc 取消）';
    status.textContent = '';
    window.addEventListener('keydown', onKey, true);
  });
  return h('span', { class: 'hk' }, btn, status);
}

/**
 * 保存一部分设置，失败时提示
 * @param {Record<string, any>} patch
 * @returns {Promise<any>} 保存后的设置（Key 已掩码）
 */
export async function savePatch(patch) {
  try {
    return await invoke('save_settings', { patch });
  } catch (e) {
    toast(errText(e));
    throw e;
  }
}
