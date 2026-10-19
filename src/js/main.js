// @ts-check
// 主窗口：左侧导航（今日 / 历史 / 设置），右侧内容

import { api } from './api.js';
import { icons } from './icons.js';
import { mountToday } from './today.js';
import { h } from './ui.js';

const content = /** @type {HTMLElement} */ (document.getElementById('content'));
const nav = /** @type {HTMLElement} */ (document.getElementById('nav'));
const banners = h('div', { class: 'banners' });

/** @type {Record<string, {label: string, icon: string, mount: (host: HTMLElement) => any}>} */
const pages = {
  today: { label: '今日', icon: icons.today, mount: (host) => mountToday(host, { banners }) },
  history: { label: '历史', icon: icons.history, mount: (host) => placeholder(host, '历史') },
  settings: { label: '设置', icon: icons.settings, mount: (host) => placeholder(host, '设置') },
};

/** @param {HTMLElement} host @param {string} name */
function placeholder(host, name) {
  host.append(h('header', { class: 'page-hd' }, h('div', { class: 'title' }, name)));
  return { refresh() {} };
}

let currentKey = '';
/** @type {any} */
let current = null;

/** @param {string} key */
export function go(key) {
  if (key === currentKey && current) return current;
  currentKey = key;
  for (const b of nav.querySelectorAll('button')) b.classList.toggle('on', b.dataset.page === key);
  content.innerHTML = '';
  const host = h('section', { class: `page ${key}` });
  content.append(host);
  current = pages[key].mount(host);
  return current;
}

for (const [key, p] of Object.entries(pages)) {
  nav.append(h('button', { 'data-page': key, html: `${p.icon}<span>${p.label}</span>`, onclick: () => go(key) }));
}

api.listen('items-changed', () => current?.refresh?.());
api.listen('tags-changed', () => current?.refresh?.());
api.listen('main:edit-item', (/** @type {{payload: string}} */ e) => go('today').editItem?.(e.payload));
window.addEventListener('focus', () => current?.refresh?.());

go('today');
