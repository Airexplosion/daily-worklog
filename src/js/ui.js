// @ts-check
// 小工具：建元素、应用内 Toast、看大图、弹出层

/**
 * @param {string} tag
 * @param {Record<string, any>} [props]
 * @param {...(Node | string | null | undefined | false)} children
 * @returns {HTMLElement}
 */
export function h(tag, props = {}, ...children) {
  const el = document.createElement(tag);
  for (const [k, v] of Object.entries(props)) {
    if (v === undefined || v === null || v === false) continue;
    if (k === 'class') el.className = v;
    else if (k === 'html') el.innerHTML = v;
    else if (k.startsWith('on')) el.addEventListener(k.slice(2).toLowerCase(), v);
    else if (k === 'style' && typeof v === 'object') Object.assign(el.style, v);
    else el.setAttribute(k, v === true ? '' : String(v));
  }
  for (const c of children) {
    if (c === null || c === undefined || c === false) continue;
    el.append(typeof c === 'string' ? document.createTextNode(c) : c);
  }
  return el;
}

let toastTimer = 0;

/**
 * 窗口右下角的小提示，可带一个操作（如"撤销"）
 * @param {string} message
 * @param {{label: string, run: () => void}} [action]
 * @param {number} [ms]
 */
export function toast(message, action, ms = 5000) {
  document.querySelector('.app-toast')?.remove();
  clearTimeout(toastTimer);
  const el = h('div', { class: 'app-toast' }, h('span', {}, message));
  if (action) {
    el.append(h('button', { class: 'link', onclick: () => { el.remove(); action.run(); } }, action.label));
  }
  document.body.append(el);
  requestAnimationFrame(() => el.classList.add('on'));
  toastTimer = window.setTimeout(() => el.remove(), ms);
}

/**
 * 看大图。多张时两边有翻页按钮、底部写"2 / 3"，← → 键也能翻（到头不循环）；Esc 或点背景关闭
 * @param {string[]} urls @param {number} [start]
 */
export function viewImages(urls, start = 0) {
  const multi = urls.length > 1;
  let i = Math.min(Math.max(start, 0), urls.length - 1);
  const img = /** @type {HTMLImageElement} */ (h('img', { alt: '截图' }));
  const prev = /** @type {HTMLButtonElement | null} */ (multi ? h('button', { class: 'nav-btn prev', title: '上一张（←）' }, '‹') : null);
  const next = /** @type {HTMLButtonElement | null} */ (multi ? h('button', { class: 'nav-btn next', title: '下一张（→）' }, '›') : null);
  const count = multi ? h('div', { class: 'count' }) : null;
  const box = h('div', { class: `viewer${multi ? ' multi' : ''}` }, prev, img, next, count);
  const show = () => {
    img.src = urls[i];
    if (count) count.textContent = `${i + 1} / ${urls.length}`;
    if (prev) prev.disabled = i === 0;
    if (next) next.disabled = i === urls.length - 1;
  };
  const step = (/** @type {number} */ d) => {
    if (i + d < 0 || i + d >= urls.length) return;
    i += d;
    show();
  };
  const close = () => {
    box.remove();
    window.removeEventListener('keydown', onKey);
  };
  const onKey = (/** @type {KeyboardEvent} */ e) => {
    if (e.key === 'Escape') close();
    else if (e.key === 'ArrowLeft') step(-1);
    else if (e.key === 'ArrowRight') step(1);
  };
  prev?.addEventListener('click', () => step(-1));
  next?.addEventListener('click', () => step(1));
  // 一张时点哪儿都关；多张时点图不关（免得翻页时手滑）
  box.addEventListener('click', (e) => { if (e.target === box || (!multi && e.target === img)) close(); });
  window.addEventListener('keydown', onKey);
  show();
  document.body.append(box);
}

/** @param {string} url */
export function viewImage(url) {
  viewImages([url]);
}

/**
 * 在 anchor 下方弹出，点外面或按 Esc 关闭
 * @param {HTMLElement} anchor
 * @param {HTMLElement} content
 * @returns {() => void} close
 */
export function popover(anchor, content) {
  document.querySelector('.popover')?.dispatchEvent(new Event('close'));
  const pop = h('div', { class: 'popover' }, content);
  document.body.append(pop);
  const r = anchor.getBoundingClientRect();
  const pw = pop.offsetWidth;
  const ph = pop.offsetHeight;
  let left = Math.min(r.left, window.innerWidth - pw - 12);
  let top = r.bottom + 6;
  if (top + ph > window.innerHeight - 8) top = Math.max(8, r.top - ph - 6);
  pop.style.left = `${Math.max(8, left)}px`;
  pop.style.top = `${top}px`;
  const onDown = (/** @type {MouseEvent} */ e) => {
    if (!pop.contains(/** @type {Node} */ (e.target)) && !anchor.contains(/** @type {Node} */ (e.target))) close();
  };
  const onKey = (/** @type {KeyboardEvent} */ e) => { if (e.key === 'Escape') close(); };
  function close() {
    pop.remove();
    document.removeEventListener('mousedown', onDown, true);
    window.removeEventListener('keydown', onKey);
  }
  pop.addEventListener('close', close);
  setTimeout(() => {
    document.addEventListener('mousedown', onDown, true);
    window.addEventListener('keydown', onKey);
  });
  return close;
}

/** @param {unknown} err */
export function errText(err) {
  return typeof err === 'string' ? err : String(/** @type {any} */ (err)?.message ?? err);
}
