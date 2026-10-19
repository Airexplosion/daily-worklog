// @ts-check
// 首次启动引导：你是谁 → 接入 AI → 你的写法 → 使用习惯，然后马上试截一张

import { row, toggle, field, select, chips, hotkeyRecorder, savePatch } from './controls.js';
import { h, toast, errText } from './ui.js';

const { invoke } = window.__TAURI__.core;

const steps = /** @type {HTMLElement} */ (document.getElementById('steps'));
const page = /** @type {HTMLElement} */ (document.getElementById('page'));
const foot = /** @type {HTMLElement} */ (document.getElementById('foot'));

const STEPS = ['你是谁', '接入 AI', '你的写法', '使用习惯'];
let current = 0;
/** @type {any} */
let view = null;
/** 第 3 步里认出来或手动加的分类 */
let pickedTags = /** @type {string[]} */ ([]);
let aiTags = new Set();

async function load() {
  view = await invoke('get_settings');
}

function renderSteps() {
  steps.innerHTML = '';
  STEPS.forEach((name, i) => {
    steps.append(h('div', { class: `step${i === current ? ' on' : ''}${i < current ? ' done' : ''}` },
      h('span', { class: 'dot' }, i < current ? '✓' : String(i + 1)), h('span', {}, name)));
  });
}

/**
 * @param {{next?: () => Promise<boolean> | boolean, nextLabel?: string, skip?: {label: string, run: () => void}, extra?: HTMLElement}} opts
 */
function footer(opts) {
  foot.innerHTML = '';
  const left = h('span', {});
  if (opts.skip) left.append(h('button', { class: 'link', onclick: opts.skip.run }, opts.skip.label));
  const right = h('span', { class: 'btns' });
  if (current > 0) right.append(h('button', { class: 'btn', onclick: () => go(current - 1) }, '上一步'));
  if (opts.extra) right.append(opts.extra);
  const next = h('button', { class: 'btn accent', id: 'next' }, opts.nextLabel || '下一步');
  next.addEventListener('click', async () => {
    if (opts.next && !(await opts.next())) return;
    go(current + 1);
  });
  right.append(next);
  foot.append(left, right);
  return next;
}

/** @param {number} i */
async function go(i) {
  current = i;
  await load();
  renderSteps();
  page.innerHTML = '';
  [stepWho, stepAi, stepStyle, stepHabits][i]();
}

function stepWho() {
  const s = view.settings;
  const preview = h('div', { class: 'preview' });
  const name = field(s.userName, () => {}, { maxlength: 10, placeholder: '比如：张三' });
  const suffix = field(s.titleSuffix, () => {}, { maxlength: 10 });
  const update = () => {
    const d = new Date();
    preview.textContent = `${name.value.trim() || '你的名字'}${d.getMonth() + 1}.${d.getDate()}${suffix.value.trim() || '工作梳理'}\n1.拼多多收入确认填写\n2.合声计划县域ppt修改`;
    next.toggleAttribute('disabled', !name.value.trim());
  };
  page.append(
    h('h2', {}, '先认识一下'),
    h('p', { class: 'lead' }, '花一分钟设好，以后每天只需要按快捷键。'),
    row('你的名字', '出现在标题最前面', name),
    row('标题后半句', '日期由程序自动生成，不补零：10.5 而不是 10.05', suffix),
    h('div', { class: 'lb' }, '成文预览'),
    preview,
  );
  const next = footer({
    next: async () => {
      await savePatch({ userName: name.value.trim(), titleSuffix: suffix.value.trim() || '工作梳理' });
      return true;
    },
  });
  name.addEventListener('input', update);
  suffix.addEventListener('input', update);
  update();
  setTimeout(() => name.focus(), 50);
}

function stepAi() {
  const s = view.settings;
  /** @type {any[]} */
  const presets = view.presets;
  const preset = presets.find((p) => p.id === s.provider) || presets[0];
  const note = h('small', { class: 'note' }, preset.note || '');
  const key = field(s.vision.apiKey, (v) => savePatch({ vision: { apiKey: v } }), { placeholder: '粘贴 API Key', spellcheck: 'false' });
  const visionModel = field(s.vision.model, (v) => savePatch({ vision: { model: v.trim() } }), { spellcheck: 'false' });
  const summaryModel = field(s.summary.model || s.vision.model, (v) => savePatch({ summary: { model: v.trim() } }), { spellcheck: 'false' });
  const baseUrl = field(s.vision.baseURL, (v) => savePatch({ vision: { baseURL: v.trim() } }), { spellcheck: 'false', placeholder: 'https://…/v1' });
  const result = h('div', { class: 'probe' });
  const help = h('button', { class: 'link', onclick: () => preset.helpUrl && invoke('open_url', { url: preset.helpUrl }) }, '没有 Key？去官网申请 →');
  help.hidden = !preset.helpUrl;
  const providerSel = select(presets.map((p) => ({ value: p.id, label: p.name })), preset.id, async (id) => {
    const p = presets.find((x) => x.id === id);
    /** @type {Record<string, any>} */
    const patch = { provider: id };
    if (id !== 'custom') {
      patch.vision = { baseURL: p.baseUrl, model: p.visionModel || p.summaryModel };
      patch.summary = { model: p.summaryModel };
      patch.sendImageToAI = p.canVision;
    }
    await savePatch(patch);
    go(1);
  });
  const test = h('button', { class: 'btn' }, '测试连接');
  test.addEventListener('click', async () => {
    // 先把还没失焦的输入存上
    for (const el of [key, visionModel, summaryModel, baseUrl]) el.dispatchEvent(new Event('change'));
    await new Promise((r) => setTimeout(r, 150));
    test.setAttribute('disabled', '');
    result.innerHTML = '';
    result.append(h('span', { class: 'spinner' }), h('span', {}, '正在连接…'));
    try {
      /** @type {any} */
      const r = await invoke('test_connection');
      result.innerHTML = '';
      const line = (/** @type {string} */ n, /** @type {any} */ p) => h('div', { class: p.ok ? 'ok' : 'bad' }, p.ok ? `✓ 能${n}　${(p.ms / 1000).toFixed(1)} 秒` : `✗ ${n}失败：${p.error}`);
      if (r.vision) result.append(line('识图', r.vision));
      result.append(line('写总结', r.summary));
    } catch (e) {
      result.textContent = errText(e);
    } finally {
      test.removeAttribute('disabled');
    }
  });
  page.append(
    h('h2', {}, '接入 AI'),
    h('p', { class: 'lead' }, '每个人用自己的 Key，费用记在自己账上。'),
    row('服务商', null, providerSel),
    preset.note ? h('div', { class: 'set-row thin' }, note) : null,
    row('API Key', null, key),
    h('div', { class: 'set-row thin' }, help),
    preset.id === 'custom' ? row('接口地址', null, baseUrl) : null,
    row('识图模型', '已按服务商选好，一般不用改', visionModel),
    row('写总结模型', null, summaryModel),
    h('div', { class: 'set-row' }, test, result),
  );
  footer({
    next: async () => {
      for (const el of [key, visionModel, summaryModel, baseUrl]) el.dispatchEvent(new Event('change'));
      return true;
    },
    skip: { label: '先跳过，只手动记录', run: () => go(2) },
  });
}

function stepStyle() {
  const s = view.settings;
  const examples = /** @type {HTMLTextAreaElement} */ (h('textarea', { class: 'input area', rows: 5, placeholder: '张三10.14工作梳理\n1.拼多多收入确认填写\n2.团播背景头图更换' }));
  examples.value = s.styleExamples;
  const status = h('span', { class: 'muted' });
  const hasKey = !!s.vision.apiKey;
  const marks = () => Object.fromEntries([...aiTags].map((t) => [t, 'AI']));
  if (!pickedTags.length) pickedTags = (view.settings.tags || []).map((/** @type {any} */ t) => t.name);
  const tagHost = h('div');
  const renderTags = () => {
    tagHost.innerHTML = '';
    tagHost.append(chips(pickedTags, (v) => { pickedTags = v; }, { max: 6, marks: marks() }));
  };
  renderTags();
  const suggest = h('button', { class: 'btn', disabled: !hasKey }, '让 AI 识别分类');
  suggest.addEventListener('click', async () => {
    suggest.setAttribute('disabled', '');
    status.textContent = '正在识别…';
    try {
      await savePatch({ styleExamples: examples.value });
      /** @type {string[]} */
      const found = await invoke('suggest_tags', { samples: examples.value });
      found.forEach((t) => aiTags.add(t));
      pickedTags = [...new Set([...pickedTags, ...found])];
      renderTags();
      status.textContent = `✓ 认出 ${found.length} 个分类`;
    } catch (e) {
      status.textContent = errText(e);
    } finally {
      suggest.removeAttribute('disabled');
    }
  });
  let allowNew = s.allowAINewTags;
  let banned = s.bannedWords;
  page.append(
    h('h2', {}, '让 AI 像你一样写'),
    h('p', { class: 'lead' }, '都可以跳过，分类也会在使用中自己长出来。'),
    h('div', { class: 'lb' }, '贴几份你以前写过的总结（AI 学口吻，顺便认出你的工作分类）'),
    examples,
    h('div', { class: 'set-row thin' }, suggest, status, hasKey ? null : h('small', { class: 'muted' }, '没接入 AI，可以手动加分类')),
    h('div', { class: 'lb' }, '你的工作分类（可删可加，带 AI 标记的是它认出来的）'),
    tagHost,
    row('遇到对不上的事，允许 AI 新建分类', '关掉则只从上面这些里选，选不出就归"其他"', toggle(allowNew, (on) => { allowNew = on; })),
    h('div', { class: 'lb' }, '不许出现的词（AI 会尽量避开，出现了会提醒你）'),
    chips(banned, (v) => { banned = v; }, { max: 8 }),
    h('small', { class: 'muted' }, '想直接改完整提示词，可以以后在「设置 → 你的写法 → 高级」里改。'),
  );
  footer({
    next: async () => {
      await savePatch({ styleExamples: examples.value, allowAINewTags: allowNew, bannedWords: banned });
      if (pickedTags.length) await invoke('add_tags', { names: pickedTags });
      return true;
    },
  });
}

function stepHabits() {
  const s = view.settings;
  page.append(
    h('h2', {}, '最后几个习惯'),
    h('p', { class: 'lead' }, '做完一件事按快捷键，下班前它会提醒你。'),
    row('截图快捷键', '点一下再按新组合即可更换', hotkeyRecorder(s.hotkey)),
    row('下班提醒时间', null, field(s.remindTime, (v) => savePatch({ remindTime: v }), { type: 'time' })),
    row('只在工作日提醒', '自动跳过法定节假日，调休上班照常提醒', toggle(s.weekdayOnly, (on) => savePatch({ weekdayOnly: on }))),
    row('开机自动启动', '在右下角托盘待命，不弹窗', toggle(s.autoStart, (on) => savePatch({ autoStart: on }))),
  );
  const later = h('button', { class: 'btn', onclick: () => invoke('finish_onboarding', { tryCapture: false }).catch((e) => toast(errText(e))) }, '完成，先不截');
  const next = footer({ nextLabel: '完成，试截一张', extra: later });
  next.addEventListener('click', (e) => {
    e.stopImmediatePropagation();
    invoke('finish_onboarding', { tryCapture: true }).catch((err) => toast(errText(err)));
  }, true);
}

go(0);
