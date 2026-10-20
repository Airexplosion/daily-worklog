// @ts-check
// "再截一张"的小条：已截 N 张，翻到下一屏后按快捷键截下一张；也可以不截了回去写说明

const { invoke } = window.__TAURI__.core;
const current = window.__TAURI__.webviewWindow.getCurrentWebviewWindow();

const text = /** @type {HTMLElement} */ (document.getElementById('text'));

/** @param {string} s */
function esc(s) {
  const d = document.createElement('div');
  d.textContent = s;
  return d.innerHTML;
}

/** @param {{count: number, hotkey: string}} v */
function render(v) {
  const keys = v.hotkey.split('+').map((k) => `<kbd>${esc(k)}</kbd>`).join('+');
  text.innerHTML = `已截 ${v.count} 张 · 翻到下一屏后按 ${keys} 截第 ${v.count + 1} 张`;
}

document.getElementById('done')?.addEventListener('click', () => invoke('bar_done'));
current.listen('bar:show', (/** @type {{payload: any}} */ ev) => render(ev.payload));
invoke('bar_current').then((v) => { if (v) render(/** @type {any} */ (v)); });
