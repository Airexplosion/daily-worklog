// @ts-check
// 今日页：日期行、待处理提示、条目清单、添加一条、从图片添加（也能拖进来、Ctrl+V）、生成今日总结、右下角"截一张"

import { api } from './api.js';
import { icons } from './icons.js';
import { renderItems, startEdit } from './itemlist.js';
import { h, toast, errText } from './ui.js';

/** @typedef {import('./api.js').DayView} DayView */

/** 当前挂着的今日页；拖入、粘贴的监听只注册一次 @type {{showing: () => boolean, dropMask: HTMLElement, dropTitle: HTMLElement} | null} */
let active = null;
let wired = false;
const isImage = (/** @type {string} */ p) => /\.(png|jpe?g|bmp|webp)$/i.test(p);
const showMsg = (/** @type {unknown} */ msg) => { if (typeof msg === 'string') toast(msg); };

function wireImageInput() {
  if (wired) return;
  wired = true;
  window.__TAURI__.webview.getCurrentWebview().onDragDropEvent((/** @type {{payload: {type: string, paths?: string[]}}} */ ev) => {
    const p = ev.payload;
    const page = active;
    if (!page) return;
    if (p.type === 'enter' && page.showing()) {
      const n = (p.paths ?? []).filter(isImage).length;
      page.dropTitle.textContent = n ? `松开，用这 ${n} 张图添加一条` : '松开，把这些文件加成一条（只认图片）';
      page.dropMask.hidden = false;
    } else if (p.type === 'leave') {
      page.dropMask.hidden = true;
    } else if (p.type === 'drop') {
      const wasShown = !page.dropMask.hidden;
      page.dropMask.hidden = true;
      if (wasShown && p.paths?.length) api.addImagePaths(p.paths).then(showMsg).catch((e) => toast(errText(e)));
    }
  });
  document.addEventListener('paste', async (e) => {
    const target = /** @type {HTMLElement | null} */ (e.target);
    if (!active?.showing() || (target && /^(INPUT|TEXTAREA)$/.test(target.tagName))) return;
    const files = [...(e.clipboardData?.items ?? [])]
      .filter((i) => i.kind === 'file' && i.type.startsWith('image/'))
      .map((i) => i.getAsFile())
      .filter((f) => !!f);
    if (!files.length) return;
    e.preventDefault();
    for (const f of /** @type {File[]} */ (files)) {
      await api.addImageBytes(new Uint8Array(await f.arrayBuffer())).then(showMsg).catch((err) => toast(errText(err)));
    }
  });
}

/**
 * @param {HTMLElement} host
 * @param {{banners: HTMLElement}} shared 顶部横幅（漏生成、更新、错误）由 main.js 管
 */
export function mountToday(host, shared) {
  /** @type {DayView | null} */
  let day = null;
  let adding = false;

  const title = h('div', { class: 'title' });
  const sub = h('div', { class: 'sub' });
  const mute = h('button', { class: 'btn subtle small', html: `${icons.bellOff}<span>今天不再提醒</span>` });
  const pendingBar = h('div', { class: 'pending-bar', hidden: true });
  const listHost = h('div', { class: 'list-host' });
  const addRow = h('div', { class: 'add-row', hidden: true });
  const genBtn = h('button', { class: 'btn accent big gen' }, '生成今日总结');
  const addBtn = h('button', { class: 'btn big', html: `${icons.add}<span>添加一条</span>` });
  const pickBtn = h('button', { class: 'btn big', title: '电脑里已有的图片，也可以直接拖进来或 Ctrl+V', html: `${icons.image}<span>从图片添加</span>` });
  const dropTitle = h('div', { class: 'drop-tt' });
  const dropMask = h('div', { class: 'drop-mask', hidden: true }, dropTitle, h('div', { class: 'drop-sub' }, '支持 png / jpg，一次最多 9 张'));
  const fab = h('button', { class: 'fab', title: '截一张并记下来（框选后回车就是记录）', html: `${icons.camera}<span>截一张</span>` });

  host.append(
    h('header', { class: 'page-hd' }, h('div', {}, title, sub), mute),
    shared.banners,
    pendingBar,
    h('div', { class: 'scroll' }, listHost, addRow),
    h('footer', { class: 'page-ft' }, addBtn, pickBtn, genBtn),
    fab,
    dropMask,
  );

  mute.addEventListener('click', async () => {
    await api.muteToday().catch((e) => toast(errText(e)));
    toast('今天不再提醒');
    refresh();
  });
  fab.addEventListener('click', () => api.captureStart('main'));
  pickBtn.addEventListener('click', () => api.pickImages().then((msg) => { if (msg) toast(msg); }).catch((e) => toast(errText(e))));

  // 电脑里已有的图片：拖到这一页上，或 Ctrl+V。监听只注册一次（切页会重新挂这一页），交给当前这一页处理
  active = { showing: () => host.isConnected && host.offsetParent !== null, dropMask, dropTitle };
  wireImageInput();
  addBtn.addEventListener('click', openAdd);
  genBtn.addEventListener('click', () => {
    if (day) window.dispatchEvent(new CustomEvent('open-summary', { detail: day.date }));
  });
  listHost.addEventListener('rerender', () => refresh());

  function openAdd() {
    if (!day || adding) return;
    adding = true;
    addRow.hidden = false;
    const input = /** @type {HTMLInputElement} */ (h('input', {
      class: 'input', maxlength: 200, placeholder: '忘了截图？直接写一句，回车保存',
    }));
    addRow.innerHTML = '';
    addRow.append(h('span', { class: 'n' }, String(day.items.length + 1)), input);
    input.focus();
    const close = () => { adding = false; addRow.hidden = true; addRow.innerHTML = ''; refresh(); };
    input.addEventListener('keydown', async (e) => {
      if (e.isComposing || e.keyCode === 229) return;
      if (e.key === 'Escape') close();
      if (e.key === 'Enter' && input.value.trim() && day) {
        const text = input.value.trim();
        close();
        await api.addItem(day.date, text).catch((err) => toast(errText(err)));
      }
    });
    input.addEventListener('blur', () => { if (!input.value.trim()) close(); });
  }

  /** 正在原地编辑或添加时不重画清单，免得把输入框刷掉；编辑结束会再刷新 */
  function editing() {
    const a = document.activeElement;
    return !!a && a.tagName === 'INPUT' && (listHost.contains(a) || addRow.contains(a));
  }

  async function refresh() {
    const date = await api.today();
    const [d, tags] = await Promise.all([api.getDay(date), api.getTags()]);
    if (editing()) return;
    day = d;
    title.textContent = d.label;
    const generated = !!d.summary;
    sub.textContent = `今日已记录 ${d.items.length} 条 · ${d.muted ? '今天不提醒' : generated ? '今日总结已生成' : `${d.remindTime} 提醒`}`;
    mute.hidden = d.muted || generated;
    const pending = d.items.filter((i) => i.status === 'pending').length;
    pendingBar.hidden = pending === 0;
    pendingBar.innerHTML = '';
    if (pending) {
      pendingBar.append(
        h('span', { html: icons.warning }),
        h('span', {}, `有 ${pending} 条还没识别出来，可以重试或手动写`),
        h('button', { class: 'link', onclick: () => api.retryAll(d.date).catch((e) => toast(errText(e))) }, '全部重试'),
      );
    }
    renderItems(listHost, { date: d.date, items: d.items, tags, hotkey: d.hotkey });
    genBtn.textContent = generated ? (d.stale ? '记录有改动 · 重新生成' : '查看今日总结') : '生成今日总结';
    genBtn.classList.toggle('stale', generated && d.stale);
  }

  /** @param {string} id */
  async function editItem(id) {
    await refresh();
    const item = day?.items.find((i) => i.id === id);
    const el = /** @type {HTMLElement | null} */ (listHost.querySelector(`[data-id="${CSS.escape(id)}"]`));
    if (item && el) {
      el.scrollIntoView({ block: 'center' });
      startEdit(el, item);
    }
  }

  refresh();
  return { refresh, editItem, get date() { return day?.date; } };
}
