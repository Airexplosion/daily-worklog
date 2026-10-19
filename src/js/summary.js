// @ts-check
// 总结视图：打字机效果生成 → 代码清洗后的全文 + 检查栏；可直接改（自动保存）、复制、重新生成（可撤销）

import { api } from './api.js';
import { icons } from './icons.js';
import { h, toast, errText } from './ui.js';

const { invoke, Channel } = window.__TAURI__.core;

/**
 * @typedef {{line: number | null, kind: string, msg: string}} Check
 * @typedef {{text: string, checks: Check[], removed: string[], skippedPending: number, notice: string | null}} SummaryResult
 */

/**
 * @param {HTMLElement} host
 * @param {string} date
 * @param {{onBack: () => void, title?: string}} opts
 */
export function mountSummary(host, date, opts) {
  const titleEl = h('div', { class: 'title' }, opts.title || '');
  const sub = h('div', { class: 'sub' });
  const doc = /** @type {HTMLTextAreaElement} */ (h('textarea', { class: 'doc', spellcheck: 'false' }));
  const checks = h('div', { class: 'checks' });
  const regen = h('button', { class: 'btn big', html: `${icons.refresh}<span>重新生成</span>` });
  const copy = h('button', { class: 'btn accent big gen', html: `${icons.copy}<span>复制到剪贴板</span>` });
  const back = h('button', { class: 'btn subtle small', html: `${icons.back}<span>回到清单</span>`, onclick: opts.onBack });

  host.append(
    h('header', { class: 'page-hd' }, h('div', {}, titleEl, sub), back),
    h('div', { class: 'summary-body' }, doc, checks),
    h('footer', { class: 'page-ft wide' }, regen, copy),
  );

  let busy = false;
  let saveTimer = 0;

  doc.addEventListener('input', () => {
    clearTimeout(saveTimer);
    saveTimer = window.setTimeout(() => {
      invoke('save_summary', { date, text: doc.value }).catch((e) => toast(errText(e)));
      sub.textContent = '已手动修改 · 已自动保存';
    }, 600);
  });

  copy.addEventListener('click', async () => {
    try {
      await invoke('copy_text', { text: doc.value });
      toast('已复制，可以直接粘到微信、企业微信或 Word');
    } catch (e) {
      toast(errText(e));
    }
  });

  regen.addEventListener('click', () => generate(true));

  /** @param {Check[]} list @param {SummaryResult | null} r */
  function showChecks(list, r) {
    checks.innerHTML = '';
    if (r?.notice) checks.append(h('div', { class: 'chk warn' }, r.notice));
    if (r && r.removed.length) {
      checks.append(h('div', { class: 'chk info' }, `已去掉 AI 多写的：${r.removed.join(' / ')}`));
    }
    if (r && r.skippedPending) {
      checks.append(h('div', { class: 'chk warn' }, `还有 ${r.skippedPending} 条没识别出来或待补充，没算进去`));
    }
    if (list.length === 0 && r && !r.notice) {
      checks.append(h('div', { class: 'chk ok', html: `${icons.check}<span>编号连续 · 无多余符号 · 没有新增人名数字</span>` }));
    }
    for (const c of list) checks.append(h('div', { class: 'chk warn', html: `${icons.warning}<span></span>` }));
    [...checks.querySelectorAll('.chk.warn span')].forEach((s, i) => { s.textContent = list[i]?.msg ?? ''; });
  }

  /** @param {boolean} isRegen */
  async function generate(isRegen) {
    if (busy) return;
    busy = true;
    regen.toggleAttribute('disabled', true);
    copy.toggleAttribute('disabled', true);
    doc.readOnly = true;
    const before = doc.value;
    doc.value = '';
    checks.innerHTML = '';
    sub.textContent = '正在生成…';
    // 打字机：收到的片段先排队，每帧吐几个字
    let queue = '';
    let streaming = true;
    const pump = () => {
      if (queue) {
        const n = Math.max(2, Math.ceil(queue.length / 12));
        doc.value += queue.slice(0, n);
        queue = queue.slice(n);
        doc.scrollTop = doc.scrollHeight;
      }
      if (streaming || queue) requestAnimationFrame(pump);
    };
    requestAnimationFrame(pump);
    const channel = new Channel();
    channel.onmessage = (/** @type {string} */ piece) => { queue += piece; };
    try {
      const day = await api.getDay(date);
      /** @type {SummaryResult} */
      const r = await invoke('generate_summary', { date, onDelta: channel });
      streaming = false;
      queue = '';
      doc.value = r.text;
      const used = day.items.length - r.skippedPending;
      const lines = r.text.split(/\r?\n/).length - 1;
      const merged = Math.max(0, used - lines);
      sub.textContent = r.notice ? '只有标题 · 已自动存档'
        : `根据 ${used} 条记录生成${merged ? ` · 合并了 ${merged} 处重复` : ''} · 已自动存档`;
      showChecks(r.checks, r);
      if (isRegen && before) {
        toast('已重新生成', {
          label: '撤销',
          run: async () => {
            try {
              doc.value = await invoke('undo_regenerate', { date });
              sub.textContent = '已撤销重新生成';
              checks.innerHTML = '';
            } catch (e) {
              toast(errText(e));
            }
          },
        });
      }
    } catch (e) {
      streaming = false;
      queue = '';
      doc.value = before;
      sub.textContent = '生成失败';
      checks.innerHTML = '';
      checks.append(h('div', { class: 'chk error', html: `${icons.warning}<span></span>` }));
      /** @type {HTMLElement} */ (checks.querySelector('.chk.error span')).textContent = errText(e);
    } finally {
      busy = false;
      regen.toggleAttribute('disabled', false);
      copy.toggleAttribute('disabled', false);
      doc.readOnly = false;
    }
  }

  (async () => {
    const day = await api.getDay(date);
    if (!opts.title) titleEl.textContent = `${day.label.split(' ')[0]} 总结`;
    if (day.summary && !day.stale) {
      doc.value = day.summary.text;
      sub.textContent = day.summary.editedAt ? '已手动修改 · 已自动保存' : '已生成 · 已自动存档';
      showChecks([], null);
    } else if (day.summary && day.stale) {
      doc.value = day.summary.text;
      sub.textContent = '生成之后记录有改动';
      checks.append(h('div', { class: 'chk warn', html: `${icons.info}<span>记录有改动，点"重新生成"更新这份总结</span>` }));
      regen.classList.add('accent-outline');
    } else {
      generate(false);
    }
  })();

  return { refresh() {} };
}
