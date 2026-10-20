// @ts-check
// 设置 → 关于 → 首次安装说明（与 docs/首次安装说明.md 内容一致，离线也能看）

import { h } from './ui.js';

/** @param {'copy' | 'record'} enter 框选后按回车做什么 */
const sections = (enter) => [
  ['安装', [
    '双击安装包，一路"下一步"即可。不需要管理员权限，装在你自己的用户目录下。',
    '少数 Win10 电脑缺少系统组件 WebView2，安装包会自动联网补装，稍等一下即可。',
  ]],
  ['弹出"Windows 已保护你的电脑"', [
    '软件暂时没有代码签名证书，Windows 会提示"未知发布者"。点「更多信息」，再点「仍要运行」。只有第一次会这样。',
  ]],
  ['360 / 火绒 / 电脑管家拦截', [
    '软件需要注册截图快捷键、截取屏幕、设置开机自启，这三样都是正常功能。',
    '弹窗时选「允许」或「信任此程序」；安装包被隔离的话，到安全软件的隔离区里恢复 DailyWorkLog 并加入信任；开机自启被拦了，到"启动项管理"里把"每日工作梳理"设为允许。',
  ]],
  ['平时怎么用', [
    '按截图快捷键（默认 Ctrl + Alt + S），拖动鼠标框选。框选后可以画圈、画箭头、打马赛克。',
    enter === 'record'
      ? '按回车记进工作梳理；想复制了发给别人，按 Ctrl+C。设置里可以把回车改回「复制」。'
      : '按回车复制，可以直接粘贴给别人；按 Tab（或点「记录」）记进工作梳理。设置里可以把回车改成「记录」。',
    '记录时补一句话（截图里没有的人名、地点、原因写在这里），按回车。想把几张图记成一条，点「再截一张」，或把图片拖进来。',
    '下班时软件会提醒你，点"生成今日总结"，再点"复制到剪贴板"，粘到微信。',
  ]],
  ['升级', [
    '发现新版本时主窗口顶部会提示，点"去下载"下载新的安装包，直接运行覆盖安装即可，记录和设置都不会丢。',
  ]],
  ['数据在哪', [
    '%APPDATA%\\DailyWorkLog\\（设置里有"打开数据文件夹"按钮）：data.json 是记录和总结，settings.json 是设置，shots 是截图原图（默认保留 90 天），backups 是每次启动的自动备份（各留 7 份），logs 是运行日志，出问题时发给发布人。',
    '卸载软件不会删除这个文件夹。',
  ]],
];

/** @param {'copy' | 'record'} [enter] */
export function showGuide(enter = 'copy') {
  const close = () => box.remove();
  const body = h('div', { class: 'guide' },
    h('div', { class: 'guide-hd' }, h('h2', {}, '首次安装说明'), h('button', { class: 'btn small', onclick: close }, '关闭')),
    ...sections(enter).map(([title, lines]) => h('section', {},
      h('h3', {}, /** @type {string} */ (title)),
      ...(/** @type {string[]} */ (lines)).map((l) => h('p', {}, l)))));
  const box = h('div', { class: 'guide-mask', onclick: (/** @type {MouseEvent} */ e) => { if (e.target === box) close(); } }, body);
  document.body.append(box);
}
