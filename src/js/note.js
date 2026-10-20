// @ts-check
// 补一句小窗：上面一排缩略图（可去掉某张、再截一张、加图片、Ctrl+V、拖进来），焦点默认在输入框；
// 回车记录，Esc 跳过（只用图生成），"不记了"什么都不留

const { invoke } = window.__TAURI__.core;
const current = window.__TAURI__.webviewWindow.getCurrentWebviewWindow();

const $ = (/** @type {string} */ id) => /** @type {HTMLElement} */ (document.getElementById(id));
const input = /** @type {HTMLInputElement} */ ($('text'));
const strip = $('strip');
const idx = $('idx');
const more = /** @type {HTMLButtonElement} */ ($('more'));
const pick = /** @type {HTMLButtonElement} */ ($('pick'));
const guide = $('guide');
const msg = $('msg');
const card = $('card');
let sending = false;
let msgTimer = 0;
let textTimer = 0;

/** @typedef {{index: number, shots: string[], note: string, max: number, message: string | null}} NoteOpen */

/** 黄色提示的位置临时换成一句说明，4 秒后换回来 @param {string} text */
function showMsg(text) {
  msg.textContent = text;
  msg.hidden = false;
  guide.hidden = true;
  clearTimeout(msgTimer);
  msgTimer = window.setTimeout(() => {
    msg.hidden = true;
    guide.hidden = false;
  }, 4000);
}

/** @param {NoteOpen} p */
function open(p) {
  sending = false;
  strip.querySelectorAll('.th').forEach((e) => e.remove());
  p.shots.forEach((url, i) => {
    const th = document.createElement('div');
    th.className = 'th';
    th.style.backgroundImage = `url("${url}")`;
    th.innerHTML = `<span class="n">${i + 1}</span>${p.shots.length > 1 ? `<button class="x" data-i="${i}" title="去掉这张">×</button>` : ''}`;
    strip.insertBefore(th, more);
  });
  const full = p.shots.length >= p.max;
  more.disabled = full;
  pick.disabled = full;
  idx.textContent = `今天第 ${p.index} 条${p.shots.length > 1 ? ` · ${p.shots.length} 张图` : ''}`;
  input.value = p.note;
  if (p.message) showMsg(p.message);
  focusInput();
}

function focusInput() {
  input.focus();
  // 窗口刚显示时焦点可能还没过来，稍后再补一次
  setTimeout(() => input.focus(), 60);
  setTimeout(() => input.focus(), 200);
}

/** 打的字马上存进草稿（去掉某张、再截一张之前也先存，免得刷新时丢字） */
function saveText() {
  clearTimeout(textTimer);
  return invoke('note_text', { text: input.value }).catch(console.error);
}

/** @param {Promise<unknown>} p 加图的结果：一张都没加成时是一句提示 */
function afterAdd(p) {
  p.then((m) => { if (typeof m === 'string') showMsg(m); }).catch((e) => showMsg(String(e)));
}

/** @param {boolean} skip */
function send(skip) {
  if (sending) return;
  sending = true;
  clearTimeout(textTimer);
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
input.addEventListener('input', () => {
  clearTimeout(textTimer);
  textTimer = window.setTimeout(saveText, 300);
});
window.addEventListener('keydown', (e) => {
  if (e.key === 'Escape' && !e.isComposing) {
    e.preventDefault();
    send(true);
  }
});
$('ok').addEventListener('click', () => send(false));
$('skip').addEventListener('click', () => send(true));
$('discard').addEventListener('click', () => invoke('note_discard'));
more.addEventListener('click', async () => {
  await saveText();
  invoke('note_more');
});
pick.addEventListener('click', async () => {
  await saveText();
  afterAdd(invoke('pick_images'));
});
strip.addEventListener('click', async (e) => {
  const x = /** @type {HTMLElement} */ (e.target).closest('.x');
  if (!(x instanceof HTMLElement)) return;
  await saveText();
  invoke('note_remove_shot', { index: Number(x.dataset.i) }).catch((err) => showMsg(String(err)));
});

// Ctrl+V：剪贴板里有图片就加进来；没有图片就照常粘贴文字
document.addEventListener('paste', async (e) => {
  const files = [...(e.clipboardData?.items ?? [])]
    .filter((i) => i.kind === 'file' && i.type.startsWith('image/'))
    .map((i) => i.getAsFile())
    .filter((f) => !!f);
  if (!files.length) return;
  e.preventDefault();
  await saveText();
  for (const f of /** @type {File[]} */ (files)) {
    await invoke('add_image_bytes', new Uint8Array(await f.arrayBuffer())).catch((err) => showMsg(String(err)));
  }
});

// 把图片文件拖到小窗上
window.__TAURI__.webview.getCurrentWebview().onDragDropEvent((/** @type {{payload: {type: string, paths?: string[]}}} */ ev) => {
  const p = ev.payload;
  card.classList.toggle('dropping', p.type === 'enter' || p.type === 'over');
  if (p.type === 'drop' && p.paths?.length) {
    saveText().then(() => afterAdd(invoke('add_image_paths', { paths: p.paths })));
  }
});

// 切去别的窗口找图时，收起背后的变暗画面，下面的窗口才看得见、拖得过来
window.addEventListener('blur', () => invoke('note_blur'));
window.addEventListener('focus', () => input.focus());

current.listen('note:open', (/** @type {{payload: NoteOpen}} */ ev) => open(ev.payload));
invoke('note_current').then((p) => { if (p) open(/** @type {NoteOpen} */ (p)); });
