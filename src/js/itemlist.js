// @ts-check
// 某一天的条目清单：双击改字、点标签改标签、删除可撤销、待处理可重试。今日页和历史页共用。

import { api } from './api.js';
import { icons } from './icons.js';
import { h, toast, viewImages, popover, errText } from './ui.js';

const MAX_TAGS = 3;
const PLACEHOLDER = '（待补充）';

/** @typedef {import('./api.js').Item} Item */
/** @typedef {import('./api.js').TagView} TagView */

/**
 * @param {HTMLElement} host
 * @param {{date: string, items: Item[], tags: TagView[], hotkey: string, compact?: boolean}} opts
 */
export function renderItems(host, opts) {
  host.innerHTML = '';
  host.dataset.date = opts.date;
  if (opts.items.length === 0) {
    const keys = opts.hotkey.split('+').map((k) => h('kbd', {}, k));
    host.append(
      h('div', { class: 'empty' },
        h('div', { class: 'empty-title' }, opts.compact ? '这天没有记录' : '今天还没记录'),
        opts.compact ? null : h('div', { class: 'empty-sub' }, '做完一件事按 ', ...keys, ' 截图，点「记录」记一条')),
    );
    return;
  }
  const list = h('div', { class: 'items' });
  opts.items.forEach((item, i) => list.append(row(item, i + 1, opts)));
  host.append(list);
}

/**
 * @param {Item} item
 * @param {number} n
 * @param {{date: string, tags: TagView[]}} opts
 */
function row(item, n, opts) {
  const low = item.status === 'done' && !item.edited && item.source === 'ai'
    && (item.text === PLACEHOLDER || (item.confidence !== null && item.confidence < 0.5));
  const el = h('div', {
    class: `item ${item.status}${low ? ' low' : ''}`,
    'data-id': item.id,
  });
  el.append(h('span', { class: 'n' }, String(n)));

  const body = h('div', { class: 'body' });
  if (item.status === 'recognizing') {
    body.append(h('div', { class: 'shimmer' }));
    if (item.note) body.append(h('div', { class: 'sub' }, `你补的话：${item.note}`));
  } else if (item.status === 'pending') {
    body.append(h('div', { class: 'text muted' }, item.note ? `你补的话：${item.note}` : '截图已保存，还没写这一条'));
    body.append(h('div', { class: 'sub warn' }, item.error || '待处理'));
  } else {
    const text = h('div', { class: 'text', title: '双击修改' }, item.text || PLACEHOLDER);
    text.addEventListener('dblclick', () => startEdit(el, item));
    body.append(text);
    if (low) body.append(h('div', { class: 'sub warn' }, 'AI 没太看懂，建议改一下'));
  }
  el.append(body);

  const side = h('div', { class: 'side' });
  if (item.status === 'pending') {
    side.append(
      h('button', { class: 'btn small', onclick: () => api.retryItem(item.id).catch((e) => toast(errText(e))) }, '重试'),
      h('button', { class: 'btn small', onclick: () => startEdit(el, item) }, '手动写'),
    );
  }
  if (item.status === 'done') {
    const tagBox = h('span', { class: 'tags' });
    for (const t of item.tags) {
      const view = opts.tags.find((x) => x.name === t);
      const chip = h('button', { class: `tag${view?.isNew ? ' new' : ''}`, title: '点击修改标签' }, view?.isNew ? `新 · ${t}` : t);
      chip.addEventListener('click', () => (view?.isNew ? newTagMenu(chip, t, opts.tags) : tagMenu(chip, item, opts.tags)));
      tagBox.append(chip);
    }
    if (item.tags.length === 0) {
      const add = h('button', { class: 'tag add', title: '加标签' }, '+ 标签');
      add.addEventListener('click', () => tagMenu(add, item, opts.tags));
      tagBox.append(add);
    }
    side.append(tagBox);
  }
  if (item.status === 'recognizing') side.append(h('span', { class: 'badge ai' }, '识别中…'));
  else side.append(h('span', { class: `badge ${item.source}` }, item.source === 'ai' ? 'AI' : '手动'));
  if (item.shots.length) side.append(thumbFor(item.shots.map(api.shotUrl)));
  side.append(h('button', {
    class: 'icon-btn del', title: '删除', html: icons.delete,
    onclick: () => remove(item),
  }));
  el.append(side);
  return el;
}

/** 5 秒撤销期内连续删除的算一组，撤销时一起恢复 */
let undoTokens = /** @type {string[]} */ ([]);
let lastDeleteAt = 0;

/** @param {Item} item */
async function remove(item) {
  try {
    const token = await api.deleteItem(item.id);
    if (Date.now() - lastDeleteAt > 5000) undoTokens = [];
    lastDeleteAt = Date.now();
    undoTokens.push(token);
    const tokens = [...undoTokens];
    toast(tokens.length > 1 ? `已删除 ${tokens.length} 条` : '已删除', {
      label: '撤销',
      run: async () => {
        undoTokens = [];
        // 后删的先恢复，原来的位置才对得上
        for (const t of tokens.reverse()) await api.undoDelete(t).catch((e) => toast(errText(e)));
      },
    });
  } catch (e) {
    toast(errText(e));
  }
}

/**
 * 原地编辑：回车或失焦保存，Esc 取消
 * @param {HTMLElement} rowEl
 * @param {Item} item
 */
export function startEdit(rowEl, item) {
  const body = /** @type {HTMLElement} */ (rowEl.querySelector('.body'));
  if (!body || body.querySelector('input')) return;
  const input = /** @type {HTMLInputElement} */ (h('input', {
    class: 'input inline', value: item.text === PLACEHOLDER ? '' : item.text, maxlength: 200,
    placeholder: item.note || '写一句这条做了什么',
  }));
  body.innerHTML = '';
  body.append(input);
  input.focus();
  input.select();
  let done = false;
  const finish = (/** @type {boolean} */ save) => {
    if (done) return;
    done = true;
    const text = input.value.trim();
    const changed = save && text && text !== item.text;
    // 先把输入框换回文字，编辑期间被推迟的刷新才能进行
    body.innerHTML = '';
    body.append(h('div', { class: 'text' }, changed ? text : item.text || PLACEHOLDER));
    if (changed) {
      api.updateItem(item.id, { text })
        .catch((e) => toast(errText(e)))
        .finally(() => rowEl.dispatchEvent(new CustomEvent('rerender', { bubbles: true })));
    } else {
      rowEl.dispatchEvent(new CustomEvent('rerender', { bubbles: true }));
    }
  };
  input.addEventListener('keydown', (e) => {
    if (e.isComposing || e.keyCode === 229) return;
    if (e.key === 'Enter') finish(true);
    if (e.key === 'Escape') finish(false);
  });
  input.addEventListener('blur', () => finish(true));
}

/**
 * 勾选已有标签（最多 3 个）或新建
 * @param {HTMLElement} anchor
 * @param {Item} item
 * @param {TagView[]} all
 */
function tagMenu(anchor, item, all) {
  const chosen = new Set(item.tags);
  const names = [...new Set([...all.map((t) => t.name), ...item.tags, '其他'])];
  const list = h('div', { class: 'tag-list' });
  const note = h('div', { class: 'pop-note' }, `最多 ${MAX_TAGS} 个`);
  const save = () => api.updateItem(item.id, { tags: [...chosen] }).catch((e) => toast(errText(e)));
  for (const name of names) {
    const box = /** @type {HTMLInputElement} */ (h('input', { type: 'checkbox' }));
    box.checked = chosen.has(name);
    box.addEventListener('change', () => {
      if (box.checked) {
        if (chosen.size >= MAX_TAGS) {
          box.checked = false;
          note.classList.add('warn');
          return;
        }
        chosen.add(name);
      } else chosen.delete(name);
      save();
    });
    list.append(h('label', { class: 'tag-opt' }, box, h('span', {}, name)));
  }
  const input = /** @type {HTMLInputElement} */ (h('input', { class: 'input', placeholder: '新标签，回车添加', maxlength: 6 }));
  input.addEventListener('keydown', (e) => {
    if (e.isComposing || e.keyCode === 229 || e.key !== 'Enter') return;
    const name = input.value.trim();
    if (!name) return;
    if (chosen.size >= MAX_TAGS) {
      note.classList.add('warn');
      return;
    }
    chosen.add(name);
    save();
    close();
  });
  const close = popover(anchor, h('div', { class: 'pop tag-pop' }, h('div', { class: 'pop-title' }, '标签'), list, input, note));
  setTimeout(() => input.focus(), 30);
}

/**
 * AI 新建的标签：保留 / 改名 / 并入已有
 * @param {HTMLElement} anchor
 * @param {string} name
 * @param {TagView[]} all
 */
function newTagMenu(anchor, name, all) {
  const others = all.filter((t) => t.name !== name && !t.isNew);
  const rename = /** @type {HTMLInputElement} */ (h('input', { class: 'input', value: name, maxlength: 6 }));
  const select = /** @type {HTMLSelectElement} */ (h('select', { class: 'input' },
    h('option', { value: '' }, '并入已有分类…'),
    ...others.map((t) => h('option', { value: t.name }, t.name))));
  const act = (/** @type {Promise<unknown>} */ p) => p.then(close).catch((e) => toast(errText(e)));
  const content = h('div', { class: 'pop new-pop' },
    h('div', { class: 'pop-title' }, `AI 新建了分类「${name}」`),
    h('div', { class: 'pop-desc' }, '现有分类里没有对得上的。不管它就自动保留；也可以现在处理：'),
    h('div', { class: 'pop-row' },
      h('button', { class: 'btn accent small', onclick: () => act(api.confirmTag(name)) }, '保留'),
      rename,
      h('button', { class: 'btn small', onclick: () => rename.value.trim() && act(api.renameTag(name, rename.value.trim())) }, '改名')),
    others.length ? h('div', { class: 'pop-row' }, select,
      h('button', { class: 'btn small', onclick: () => select.value && act(api.renameTag(name, select.value)) }, '并入')) : null,
  );
  const close = popover(anchor, content);
}

/**
 * 缩略图：一张就是一张；多张叠成一摞、右下角标张数。点开看大图，多张可以左右翻
 * @param {string[]} urls
 */
function thumbFor(urls) {
  // 截图超过保留天数会被清理，找不到时换成灰色占位
  const gone = () => h('span', { class: 'thumb gone', title: '截图已清理' });
  if (urls.length === 1) {
    const img = h('img', { class: 'thumb', src: urls[0], alt: '', title: '点击看大图', onclick: () => viewImages(urls) });
    img.addEventListener('error', () => img.replaceWith(gone()));
    return img;
  }
  const back = h('img', { class: 'thumb back', src: urls[1], alt: '' });
  const front = h('img', { class: 'thumb front', src: urls[0], alt: '' });
  back.addEventListener('error', () => back.remove());
  front.addEventListener('error', () => {
    const g = gone();
    g.classList.add('front');
    front.replaceWith(g);
  });
  return h('span', { class: 'thumb-stack', title: `${urls.length} 张图，点击看大图`, onclick: () => viewImages(urls) },
    back, front, h('span', { class: 'cnt' }, String(urls.length)));
}
