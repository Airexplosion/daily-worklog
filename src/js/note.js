// @ts-check
// 补充说明框：焦点默认在输入框；回车记录，Esc 跳过（只用截图生成）

const { invoke } = window.__TAURI__.core;
const current = window.__TAURI__.webviewWindow.getCurrentWebviewWindow();

const input = /** @type {HTMLInputElement} */ (document.getElementById('text'));
const thumb = /** @type {HTMLElement} */ (document.getElementById('thumb'));
const idx = /** @type {HTMLElement} */ (document.getElementById('idx'));
let sending = false;

/** @param {{index: number, shots: string[], note: string, max: number, message: string | null}} p */
function open(p) {
  sending = false;
  thumb.style.backgroundImage = `url("${p.shots[0] ?? ''}")`;
  idx.textContent = `今天第 ${p.index} 条`;
  input.value = p.note;
  focusInput();
}

function focusInput() {
  input.focus();
  // 窗口刚显示时焦点可能还没过来，稍后再补一次
  setTimeout(() => input.focus(), 60);
  setTimeout(() => input.focus(), 200);
}

/** @param {boolean} skip */
function send(skip) {
  if (sending) return;
  sending = true;
  const cmd = skip ? invoke('note_skip') : invoke('note_submit', { text: input.value });
  cmd.catch((err) => {
    sending = false;
    console.error(err);
  });
}

input.addEventListener('keydown', (e) => {
  // 输入法选字时的回车不算提交
  if (e.isComposing || e.keyCode === 229) return;
  if (e.key === 'Enter') {
    e.preventDefault();
    send(false);
  }
});
window.addEventListener('keydown', (e) => {
  if (e.key === 'Escape' && !e.isComposing) {
    e.preventDefault();
    send(true);
  }
});
document.getElementById('ok')?.addEventListener('click', () => send(false));
document.getElementById('skip')?.addEventListener('click', () => send(true));

current.listen('note:open', (/** @type {{payload: any}} */ ev) => open(ev.payload));
window.addEventListener('focus', () => input.focus());
invoke('note_current').then((p) => { if (p) open(/** @type {any} */ (p)); });
