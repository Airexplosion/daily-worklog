// @ts-check
// 主窗口：左侧导航（今日 / 历史 / 设置），右侧内容

import { api } from './api.js';
import { icons } from './icons.js';
import { mountToday } from './today.js';
import { mountSummary } from './summary.js';
import { mountHistory } from './history.js';
import { mountSettings } from './settings.js';
import { h } from './ui.js';

const content = /** @type {HTMLElement} */ (document.getElementById('content'));
const nav = /** @type {HTMLElement} */ (document.getElementById('nav'));
const banners = h('div', { class: 'banners' });

/** @type {Record<string, {label: string, icon: string, mount: (host: HTMLElement) => any}>} */
const pages = {
  today: { label: '今日', icon: icons.today, mount: (host) => mountToday(host, { banners }) },
  history: { label: '历史', icon: icons.history, mount: (host) => mountHistory(host) },
  settings: { label: '设置', icon: icons.settings, mount: (host) => mountSettings(host) },
};

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

/** 顶部横幅：热键冲突、数据恢复、漏生成。只在今日页显示 */
async function renderBanners() {
  /** @type {any} */
  const s = await window.__TAURI__.core.invoke('get_status');
  banners.innerHTML = '';
  const invoke = window.__TAURI__.core.invoke;
  if (s.hotkeyError) {
    banners.append(h('div', { class: 'banner error' },
      h('span', { class: 'tx' }, `截图快捷键 ${s.hotkeyError} 用不了，可能被别的软件占用了`),
      h('button', { class: 'btn small', onclick: () => go('settings') }, '去设置里换一个')));
  }
  if (s.dataLost) {
    banners.append(h('div', { class: 'banner error' },
      h('span', { class: 'tx' }, '数据文件损坏，而且没有可用的备份，只能从空白开始', h('small', {}, '坏掉的文件已另存在数据文件夹里，没有删除')),
      h('button', { class: 'btn small', onclick: () => invoke('dismiss_recovered').then(renderBanners) }, '知道了')));
  } else if (s.recoveredFrom) {
    const m = /data-(\d{4})(\d{2})(\d{2})-(\d{2})(\d{2})/.exec(s.recoveredFrom);
    const when = m ? `${Number(m[2])}月${Number(m[3])}日 ${m[4]}:${m[5]}` : s.recoveredFrom;
    banners.append(h('div', { class: 'banner' },
      h('span', { class: 'tx' }, `数据文件损坏，已从 ${when} 的备份恢复`, h('small', {}, '那之后记的条目可能没了，请检查一下')),
      h('button', { class: 'btn small', onclick: () => invoke('dismiss_recovered').then(renderBanners) }, '知道了')));
  }
  /** @type {any} */
  const up = await invoke('pending_update');
  if (up) {
    banners.append(h('div', { class: 'banner info' },
      h('span', { class: 'tx' }, `有新版本 v${up.latest}`, h('small', {}, `GitHub 打不开？找${up.publisher}要安装包`)),
      h('button', { class: 'btn small', onclick: () => invoke('snooze_update', { version: up.latest }).then(renderBanners) }, '稍后'),
      h('button', { class: 'btn small accent', onclick: () => invoke('open_url', { url: up.url }) }, '去下载')));
  }
  if (s.missed) {
    banners.append(h('div', { class: 'banner' },
      h('span', { class: 'tx' }, `${s.missed.label}有 ${s.missed.count} 条记录还没生成总结`, h('small', {}, '那天提醒的时候电脑可能没开，提醒没弹出来')),
      h('button', { class: 'btn small', onclick: () => invoke('dismiss_missed', { date: s.missed.date }).then(renderBanners) }, '忽略'),
      h('button', { class: 'btn small accent', onclick: () => window.dispatchEvent(new CustomEvent('open-summary', { detail: s.missed.date })) }, '现在生成')));
  }
}

api.listen('items-changed', () => { current?.refresh?.(); renderBanners(); });
api.listen('summary-generated', () => renderBanners());
api.listen('update-available', () => renderBanners());
api.listen('main:open-summary', async () => {
  go('today');
  window.dispatchEvent(new CustomEvent('open-summary', { detail: await api.today() }));
});
api.listen('main:remind', () => { go('today'); current?.refresh?.(); });
api.listen('tags-changed', () => current?.refresh?.());
api.listen('main:edit-item', (/** @type {{payload: string}} */ e) => go('today').editItem?.(e.payload));
window.addEventListener('focus', () => { current?.refresh?.(); renderBanners(); });

/** 总结视图挂在"今日"（或"历史"）下面，返回时回到原来的页面 */
window.addEventListener('open-summary', async (/** @type {any} */ e) => {
  const date = e.detail;
  const isToday = date === (await api.today());
  const from = currentKey.split(':')[0] || 'today';
  content.innerHTML = '';
  const host = h('section', { class: 'page summary' });
  content.append(host);
  currentKey = `${from}:summary`;
  current = mountSummary(host, date, {
    title: isToday ? '今日总结' : undefined,
    onBack: () => { currentKey = ''; go(from); },
  });
});

go('today');
renderBanners();
