// @ts-check
// 设置页：改了就自动保存（Win11 设置的习惯）

import { section, row, toggle, field, select, chips, hotkeyRecorder, savePatch } from './controls.js';
import { icons } from './icons.js';
import { h, toast, errText } from './ui.js';
import { showGuide } from './guide.js';

const { invoke } = window.__TAURI__.core;

/** @typedef {{id: string, name: string, baseUrl: string, visionModel: string, summaryModel: string, helpUrl: string, canVision: boolean, note: string}} Preset */

/** @param {HTMLElement} host */
export function mountSettings(host) {
  const saved = h('div', { class: 'sub' }, '改了会自动保存');
  const body = h('div', { class: 'scroll settings' });
  host.append(h('header', { class: 'page-hd' }, h('div', {}, h('div', { class: 'title' }, '设置'), saved)), body);

  const flash = () => {
    saved.textContent = '已保存';
    setTimeout(() => { saved.textContent = '改了会自动保存'; }, 1500);
  };
  /** @param {Record<string, any>} patch */
  const save = (patch) => savePatch(patch).then((s) => { flash(); return s; });

  async function render() {
    /** @type {any} */
    const view = await invoke('get_settings');
    /** @type {any} */
    const s = view.settings;
    /** @type {Preset[]} */
    const presets = view.presets;
    body.innerHTML = '';

    // 1. 你是谁
    const preview = h('div', { class: 'preview' });
    const updatePreview = () => {
      const d = new Date();
      preview.textContent = `${nameInput.value || '你的名字'}${d.getMonth() + 1}.${d.getDate()}${suffixInput.value}\n1.（第一件事）\n2.（第二件事）`;
    };
    const nameInput = field(s.userName, (v) => save({ userName: v.trim() }), { maxlength: 10 });
    const suffixInput = field(s.titleSuffix, (v) => save({ titleSuffix: v.trim() || '工作梳理' }), { maxlength: 10 });
    nameInput.addEventListener('input', updatePreview);
    suffixInput.addEventListener('input', updatePreview);
    updatePreview();
    body.append(section('你是谁',
      row('名字', '出现在标题最前面', nameInput),
      row('标题后半句', '日期由程序自动生成，不补零：10.5 而不是 10.05', suffixInput),
      h('div', { class: 'set-row' }, h('div', { class: 'set-label' }, h('div', {}, '成文预览')), preview),
    ));

    // 2. AI 接口
    const preset = presets.find((p) => p.id === s.provider) || presets[presets.length - 1];
    const keyInput = field(s.vision.apiKey, (v) => save({ vision: { apiKey: v } }).then(render), { placeholder: '粘贴你的 API Key', spellcheck: 'false' });
    const reveal = h('button', { class: 'btn small subtle', onclick: async () => { keyInput.value = await invoke('reveal_api_key', { which: 'vision' }); } }, '显示');
    const result = h('div', { class: 'probe' });
    const testBtn = h('button', { class: 'btn', onclick: () => testConnection(testBtn, result) }, '测试连接');
    const sendImage = toggle(s.sendImageToAI, (on) => save({ sendImageToAI: on }));
    body.append(section('AI 接口',
      row('服务商', preset.note || null, select(presets.map((p) => ({ value: p.id, label: p.name })), preset.id, async (id) => {
        const p = /** @type {Preset} */ (presets.find((x) => x.id === id));
        /** @type {Record<string, any>} */
        const patch = { provider: id };
        if (id !== 'custom') {
          patch.vision = { baseURL: p.baseUrl, model: p.visionModel || p.summaryModel };
          patch.summary = { model: p.summaryModel };
          patch.sendImageToAI = p.canVision;
        }
        await save(patch);
        render();
      })),
      row('API Key', '只存在你自己电脑上', keyInput, reveal),
      preset.helpUrl ? h('div', { class: 'set-row thin' }, h('button', { class: 'link', onclick: () => invoke('open_url', { url: preset.helpUrl }) }, '没有 Key？去官网申请 →')) : null,
      row('接口地址', null, field(s.vision.baseURL, (v) => save({ vision: { baseURL: v.trim() } }), { spellcheck: 'false' })),
      row('识图模型', '能看图的模型', field(s.vision.model, (v) => save({ vision: { model: v.trim() } }), { spellcheck: 'false' })),
      row('写总结模型', '只处理文字，可以用便宜的模型', field(s.summary.model || s.vision.model, (v) => save({ summary: { model: v.trim() } }), { spellcheck: 'false' })),
      row('截图内容发送给 AI', '关掉后只根据你补的那句话生成，不上传任何图片', sendImage),
      h('div', { class: 'set-row' }, h('div', { class: 'set-label' }, h('div', {}, '检查一下')), h('div', { class: 'set-ctl col' }, testBtn, result)),
      advancedSummary(s, save),
    ));

    // 3. 你的写法
    const examples = /** @type {HTMLTextAreaElement} */ (h('textarea', { class: 'input area', rows: 5, placeholder: '把以前写过的几份工作梳理贴在这里，AI 会学你的口吻' }));
    examples.value = s.styleExamples;
    examples.addEventListener('change', () => save({ styleExamples: examples.value }));
    body.append(section('你的写法',
      row('以前写过的总结', 'AI 学口吻用，可以不填', examples),
      row('不许出现的词', 'AI 会尽量避开；出现了会在检查栏提醒', chips(s.bannedWords, (v) => save({ bannedWords: v }), { max: 8 })),
      row('允许 AI 新建分类', '关掉则只从现有分类里选，选不出就归"其他"', toggle(s.allowAINewTags, (on) => save({ allowAINewTags: on }))),
      advancedPrompts(s, view.defaultPrompts, save),
    ));

    // 4. 分类管理
    body.append(await tagSection(render));

    // 5. 使用习惯
    const time = field(s.remindTime, (v) => save({ remindTime: v }).then(render), { type: 'time' });
    const days = field(String(s.shotsRetentionDays), (v) => save({ shotsRetentionDays: Number(v) || 90 }), { type: 'number', min: 1, max: 3650 });
    body.append(section('使用习惯',
      row('截图快捷键', '点一下，再按新的组合', hotkeyRecorder(s.hotkey, () => flash())),
      row('框选后按回车', '另一种操作：Tab 记录，Ctrl+C 复制', select([{ value: 'copy', label: '复制' }, { value: 'record', label: '记录（进补一句）' }], s.enterAction ?? 'copy', (v) => save({ enterAction: v }))),
      row('下班提醒时间', null, time),
      row('只在工作日提醒', view.holidayStale ? '节假日数据过期（没有今年的），暂时按周一到周五提醒，请更新软件' : '自动跳过法定节假日，调休上班照常提醒', toggle(s.weekdayOnly, (on) => save({ weekdayOnly: on }))),
      row('开机自动启动', '在右下角托盘待命，不弹窗', toggle(s.autoStart, (on) => save({ autoStart: on }))),
      row('截图保留天数', '超过的原图自动删除，文字记录一直保留', days),
    ));

    // 6. 数据与关于
    body.append(section('数据与关于',
      row('数据文件夹', view.dataDir, h('button', { class: 'btn', html: `${icons.folder}<span>打开</span>`, onclick: () => invoke('open_data_dir').catch((e) => toast(errText(e))) })),
      row('版本', `每日工作梳理 v${view.version}`, updateChecker()),
      row('首次安装说明', '装不上、被拦截、数据在哪，都写在这里', h('button', { class: 'btn', onclick: showGuide }, '查看')),
    ));
  }

  render();
  return { refresh() {} };
}

/** 设置页里手动检查更新 */
function updateChecker() {
  const out = h('span', { class: 'muted' });
  const btn = h('button', { class: 'btn' }, '检查更新');
  btn.addEventListener('click', async () => {
    btn.setAttribute('disabled', '');
    out.textContent = '正在检查…';
    try {
      /** @type {any} */
      const r = await invoke('check_update', { manual: true });
      out.innerHTML = '';
      if (r.newer) {
        out.append(`有新版本 v${r.latest} `, h('button', { class: 'link', onclick: () => invoke('open_url', { url: r.url }) }, '去下载'));
      } else {
        out.textContent = r.latest ? '已经是最新版本' : 'GitHub 上还没有发布过版本';
      }
    } catch (e) {
      out.textContent = errText(e);
    } finally {
      btn.removeAttribute('disabled');
    }
  });
  return h('span', { class: 'set-ctl' }, out, btn);
}

/**
 * @param {HTMLElement} btn
 * @param {HTMLElement} out
 */
async function testConnection(btn, out) {
  btn.setAttribute('disabled', '');
  out.innerHTML = '';
  out.append(h('span', { class: 'spinner' }), h('span', {}, '正在连接…'));
  try {
    /** @type {any} */
    const r = await invoke('test_connection');
    out.innerHTML = '';
    const line = (/** @type {string} */ name, /** @type {any} */ p) => h('div', { class: p.ok ? 'ok' : 'bad' },
      p.ok ? `✓ 能${name}　用时 ${(p.ms / 1000).toFixed(1)} 秒` : `✗ ${name}失败：${p.error}`);
    if (r.vision) out.append(line('识图', r.vision));
    else out.append(h('div', { class: 'muted' }, '不发截图，没测识图'));
    out.append(line('写总结', r.summary));
  } catch (e) {
    out.textContent = errText(e);
  } finally {
    btn.removeAttribute('disabled');
  }
}

/**
 * 汇总单独配置（高级）
 * @param {any} s
 * @param {(p: Record<string, any>) => Promise<any>} save
 */
function advancedSummary(s, save) {
  const box = h('details', { class: 'adv' }, h('summary', {}, '高级：写总结用另一家服务商'));
  box.append(
    row('单独配置', '关掉时和识图共用接口地址和 Key', toggle(s.separateSummary, (on) => save({ separateSummary: on }))),
    row('接口地址', null, field(s.summary.baseURL, (v) => save({ summary: { baseURL: v.trim() } }), { spellcheck: 'false' })),
    row('API Key', null, field(s.summary.apiKey, (v) => save({ summary: { apiKey: v } }), { spellcheck: 'false' })),
  );
  return box;
}

/**
 * 完整提示词（高级）：留空 = 用默认
 * @param {any} s
 * @param {{vision: string, summary: string}} defaults
 * @param {(p: Record<string, any>) => Promise<any>} save
 */
function advancedPrompts(s, defaults, save) {
  const box = h('details', { class: 'adv' }, h('summary', {}, '高级：直接改完整提示词'));
  /** @param {'vision' | 'summary'} key @param {string} label */
  const editor = (key, label) => {
    const ta = /** @type {HTMLTextAreaElement} */ (h('textarea', { class: 'input area mono', rows: 10 }));
    ta.value = s.promptOverrides[key] || defaults[key];
    ta.addEventListener('change', () => save({ promptOverrides: { [key]: ta.value.trim() === defaults[key].trim() ? null : ta.value } }));
    const reset = h('button', { class: 'btn small', onclick: () => { ta.value = defaults[key]; save({ promptOverrides: { [key]: null } }); } }, '恢复默认');
    return h('div', { class: 'prompt-ed' }, h('div', { class: 'prompt-hd' }, h('span', {}, label), reset), ta);
  };
  box.append(
    h('div', { class: 'adv-note' }, '{姓名}{禁用词}{现有标签}{新建标签规则}{口吻范例段} 会被自动替换。要求 AI 按固定格式返回结果的那部分由程序自动加在最后，这里改不了。'),
    editor('vision', '识图提示词'),
    editor('summary', '写总结提示词'),
  );
  return box;
}

/** @param {() => void} rerender */
async function tagSection(rerender) {
  /** @type {{name: string, count: number, byAi: boolean, isNew: boolean}[]} */
  const list = await invoke('tag_usage');
  const rows = list.map((t) => {
    const others = list.filter((x) => x.name !== t.name);
    const merge = /** @type {HTMLSelectElement} */ (h('select', { class: 'input small' },
      h('option', { value: '' }, '并入…'), ...others.map((o) => h('option', { value: o.name }, o.name))));
    merge.addEventListener('change', () => merge.value && act(invoke('rename_tag', { from: t.name, to: merge.value })));
    const act = (/** @type {Promise<unknown>} */ p) => p.then(rerender).catch((e) => toast(errText(e)));
    const nameEl = h('span', { class: `tag${t.isNew ? ' new' : ''}` }, t.isNew ? `新 · ${t.name}` : t.name);
    // 原地改名：回车保存，Esc 取消；新名字已存在就是合并
    const rename = () => {
      const input = /** @type {HTMLInputElement} */ (h('input', { class: 'input small', value: t.name, maxlength: 6 }));
      nameEl.replaceWith(input);
      input.focus();
      input.select();
      input.addEventListener('keydown', (e) => {
        if (e.isComposing || e.keyCode === 229) return;
        const to = input.value.trim();
        if (e.key === 'Enter' && to && to !== t.name) act(invoke('rename_tag', { from: t.name, to }));
        if (e.key === 'Escape') rerender();
      });
    };
    return h('div', { class: 'tag-row' },
      nameEl,
      t.byAi ? h('span', { class: 'badge ai' }, 'AI 建的') : null,
      h('span', { class: 'count' }, `用了 ${t.count} 次`),
      h('span', { class: 'acts' },
        h('button', { class: 'btn small subtle', onclick: rename }, '改名'),
        others.length ? merge : null,
        h('button', { class: 'btn small subtle danger', onclick: () => act(invoke('delete_tag', { name: t.name })) }, '删除')));
  });
  const add = /** @type {HTMLInputElement} */ (h('input', { class: 'input', placeholder: '新分类名，回车添加', maxlength: 6 }));
  add.addEventListener('keydown', (e) => {
    if (e.isComposing || e.keyCode === 229 || e.key !== 'Enter' || !add.value.trim()) return;
    invoke('add_tags', { names: [add.value.trim()] }).then(rerender).catch((err) => toast(errText(err)));
  });
  return section('分类管理',
    list.length ? h('div', { class: 'tag-rows' }, ...rows) : h('div', { class: 'set-row thin muted' }, '还没有分类。AI 识图时会自动建，也可以在这里手动加。'),
    h('div', { class: 'set-row thin' }, add),
  );
}
