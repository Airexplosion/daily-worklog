// @ts-check
// 历史页：左边按月列出有记录的日子，右边是那天的总结（可复制、可补生成）和原始记录（可改）

import { api } from './api.js';
import { icons } from './icons.js';
import { renderItems } from './itemlist.js';
import { h, toast, errText } from './ui.js';

const { invoke } = window.__TAURI__.core;

/** @typedef {{date: string, label: string, count: number, generated: boolean}} DayListItem */

/** 回到历史页时停在上次看的那天 */
let lastSelected = '';

/** @param {HTMLElement} host */
export function mountHistory(host) {
  const sub = h('div', { class: 'sub' });
  const list = h('div', { class: 'days' });
  const detail = h('div', { class: 'detail' });
  host.append(
    h('header', { class: 'page-hd' }, h('div', {}, h('div', { class: 'title' }, '历史'), sub)),
    h('div', { class: 'history-body' }, list, detail),
  );

  /** @type {DayListItem[]} */
  let days = [];

  async function refresh() {
    const a = document.activeElement;
    if (a && a.tagName === 'INPUT' && detail.contains(a)) return;
    days = await invoke('list_days');
    const today = await api.today();
    const month = today.slice(0, 7);
    const thisMonth = days.filter((d) => d.date.startsWith(month));
    const missing = thisMonth.filter((d) => !d.generated && d.date !== today).length;
    sub.textContent = days.length
      ? `本月 ${thisMonth.length} 天有记录${missing ? `，${missing} 天没生成总结` : ''}`
      : '还没有任何记录';
    renderList();
    if (!days.some((d) => d.date === lastSelected)) lastSelected = days[0]?.date ?? '';
    await renderDetail();
  }

  function renderList() {
    list.innerHTML = '';
    let month = '';
    for (const d of days) {
      const m = d.date.slice(0, 7);
      if (m !== month) {
        month = m;
        list.append(h('div', { class: 'month' }, `${m.slice(0, 4)} 年 ${Number(m.slice(5))} 月`));
      }
      list.append(h('button', {
        class: `day${d.date === lastSelected ? ' on' : ''}`,
        onclick: () => { lastSelected = d.date; renderList(); renderDetail(); },
      }, h('span', {}, d.label), h('span', { class: `st ${d.generated ? 'g' : 'p'}` }, d.generated ? '已生成' : '未生成')));
    }
  }

  async function renderDetail() {
    detail.innerHTML = '';
    if (!lastSelected) {
      detail.append(h('div', { class: 'empty' }, h('div', { class: 'empty-title' }, '这里会按天列出你记过的工作')));
      return;
    }
    const date = lastSelected;
    const [day, tags] = await Promise.all([api.getDay(date), api.getTags()]);
    if (date !== lastSelected) return;
    const open = () => window.dispatchEvent(new CustomEvent('open-summary', { detail: date }));

    const sumHead = h('div', { class: 'lb' }, h('span', {}, `${day.label} · 总结`));
    const sumBox = h('div', { class: 'doc2' });
    if (day.summary) {
      sumBox.textContent = day.summary.text;
      sumHead.append(
        h('button', { class: 'btn subtle small', html: `${icons.edit}<span>${day.stale ? '记录有改动 · 重新生成' : '修改 / 重新生成'}</span>`, onclick: open }),
        h('button', {
          class: 'btn small', html: `${icons.copy}<span>复制</span>`,
          onclick: () => invoke('copy_text', { text: day.summary?.text ?? '' }).then(() => toast('已复制')).catch((e) => toast(errText(e))),
        }),
      );
    } else {
      sumBox.classList.add('none');
      sumBox.append(h('span', {}, '这天还没生成总结'), h('button', { class: 'btn accent small', onclick: open }, '生成这天的总结'));
    }

    const itemsHost = h('div', { class: 'list-host' });
    itemsHost.addEventListener('rerender', () => renderDetail());
    renderItems(itemsHost, { date, items: day.items, tags, hotkey: day.hotkey, compact: true });

    const addBtn = h('button', { class: 'btn subtle small', html: `${icons.add}<span>补一条</span>` });
    addBtn.addEventListener('click', () => {
      const input = /** @type {HTMLInputElement} */ (h('input', { class: 'input', maxlength: 200, placeholder: '这天漏记的事，回车保存' }));
      addBtn.replaceWith(input);
      input.focus();
      input.addEventListener('keydown', (e) => {
        if (e.isComposing || e.keyCode === 229) return;
        if (e.key === 'Escape') renderDetail();
        if (e.key === 'Enter' && input.value.trim()) {
          const text = input.value.trim();
          input.blur();
          api.addItem(date, text).catch((err) => toast(errText(err)));
        }
      });
      input.addEventListener('blur', () => { if (!input.value.trim()) renderDetail(); });
    });

    detail.append(
      sumHead, sumBox,
      h('div', { class: 'lb' }, h('span', {}, `原始记录 ${day.items.length} 条`), addBtn),
      itemsHost,
    );
  }

  refresh();
  return { refresh };
}
