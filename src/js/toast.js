// @ts-check
// 右下角提示：识别中 → 已生成 / 已保存待处理。约 5 秒自动消失，鼠标悬停时不消失

const { invoke } = window.__TAURI__.core;
const current = window.__TAURI__.webviewWindow.getCurrentWebviewWindow();

const $ = (/** @type {string} */ id) => /** @type {HTMLElement} */ (document.getElementById(id));
const card = $('card');
let hideTimer = 0;
let hovering = false;
let currentId = '';
let lastDelay = 5000;

/** @param {number} ms */
function scheduleHide(ms) {
  lastDelay = ms;
  clearTimeout(hideTimer);
  if (hovering) return;
  hideTimer = window.setTimeout(hide, ms);
}

function hide() {
  document.body.classList.remove('on');
  setTimeout(() => invoke('toast_hide'), 180);
}

/** @param {string} s */
function esc(s) {
  const d = document.createElement('div');
  d.textContent = s;
  return d.innerHTML;
}

/**
 * @param {{kind: string, id: string, index: number, text: string, tags: string[], newTags: string[],
 *          note: string, message: string, lowConfidence: boolean, shots: number}} p
 */
function render(p) {
  currentId = p.id;
  const icon = $('icon');
  const title = $('title');
  const line1 = $('line1');
  const tags = $('tags');
  $('low').hidden = !(p.kind === 'done' && p.lowConfidence);
  line1.className = 'ds';
  line1.hidden = false;
  tags.innerHTML = '';
  if (p.kind === 'recognizing') {
    icon.className = 'spinner';
    icon.textContent = '';
    title.textContent = `已记录第 ${p.index} 条${p.shots > 1 ? ` · ${p.shots} 张图` : ''} · AI 识别中…`;
    line1.className = 'ds muted';
    line1.textContent = p.note ? `你补的话：${p.note}` : '没补充说明，只用截图生成';
    scheduleHide(30000);
  } else if (p.kind === 'done') {
    icon.className = 'ok';
    icon.textContent = '✓';
    title.textContent = `第 ${p.index} 条已生成`;
    line1.textContent = p.text;
    tags.innerHTML = p.tags
      .map((t) => (p.newTags.includes(t) ? `<span class="tag new">新 · ${esc(t)}</span>` : `<span class="tag">${esc(t)}</span>`))
      .join('') + '<button class="link" id="edit">改一下</button>';
    scheduleHide(5000);
  } else if (p.kind === 'copied') {
    // 复制、另存：闪一下就走
    icon.className = 'ok';
    icon.textContent = '✓';
    title.textContent = p.text;
    line1.textContent = '';
    line1.hidden = true;
    scheduleHide(1500);
  } else if (p.kind === 'info') {
    icon.className = 'info';
    icon.textContent = 'i';
    title.textContent = p.text;
    line1.className = 'ds muted';
    line1.textContent = p.message;
    scheduleHide(5000);
  } else {
    icon.className = 'warn';
    icon.textContent = '!';
    title.textContent = `第 ${p.index} 条已保存`;
    line1.className = 'ds muted';
    line1.textContent = p.message;
    tags.innerHTML = '<button class="link" id="edit">去写一句</button>';
    scheduleHide(6000);
  }
  document.getElementById('edit')?.addEventListener('click', () => invoke('toast_open_item', { id: currentId }));
  document.body.classList.add('on');
}

card.addEventListener('mouseenter', () => {
  hovering = true;
  clearTimeout(hideTimer);
});
card.addEventListener('mouseleave', () => {
  hovering = false;
  scheduleHide(Math.min(lastDelay, 2500));
});
$('close').addEventListener('click', hide);

current.listen('toast:show', (/** @type {{payload: any}} */ ev) => render(ev.payload));
