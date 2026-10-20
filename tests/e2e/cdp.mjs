// 通过 WebView2 远程调试协议驱动程序窗口：事件直接发进网页，不碰系统鼠标键盘
import fs from 'node:fs';

export async function connect(urlPart, port = 9222) {
  let targets = [];
  for (let i = 0; i < 50; i++) {
    try {
      targets = await (await fetch(`http://127.0.0.1:${port}/json`)).json();
      if (targets.some((t) => matches(t.url, urlPart))) break;
    } catch {}
    await sleep(200);
  }
  const t = targets.find((x) => matches(x.url, urlPart) && x.type === 'page');
  if (!t) throw new Error(`找不到页面 ${urlPart}：${targets.map((x) => x.url).join(', ')}`);
  const ws = new WebSocket(t.webSocketDebuggerUrl);
  await new Promise((r, j) => { ws.onopen = r; ws.onerror = j; });
  let id = 0;
  const waiting = new Map();
  ws.onmessage = (m) => {
    const msg = JSON.parse(m.data);
    if (msg.method) page.events.push(msg);
    if (msg.id && waiting.has(msg.id)) {
      const { resolve, reject } = waiting.get(msg.id);
      waiting.delete(msg.id);
      msg.error ? reject(new Error(JSON.stringify(msg.error))) : resolve(msg.result);
    }
  };
  const send = (method, params = {}) => new Promise((resolve, reject) => {
    const n = ++id;
    waiting.set(n, { resolve, reject });
    ws.send(JSON.stringify({ id: n, method, params }));
  });
  const page = {
    events: [],
    send,
    async eval(expr) {
      const r = await send('Runtime.evaluate', { expression: expr, awaitPromise: true, returnByValue: true });
      if (r.exceptionDetails) throw new Error(`页面脚本出错：${r.exceptionDetails.exception?.description || r.exceptionDetails.text}`);
      return r.result.value;
    },
    async center(sel) {
      const c = await page.eval(`(() => { const el = document.querySelector(${JSON.stringify(sel)}); if (!el) return null;
        el.scrollIntoView({block: 'center'}); const r = el.getBoundingClientRect(); return {x: r.left + r.width / 2, y: r.top + r.height / 2}; })()`);
      if (!c) throw new Error(`找不到元素 ${sel}`);
      return c;
    },
    async click(sel, count = 1) {
      const { x, y } = await page.center(sel);
      await send('Input.dispatchMouseEvent', { type: 'mouseMoved', x, y });
      for (let i = 1; i <= count; i++) {
        await send('Input.dispatchMouseEvent', { type: 'mousePressed', x, y, button: 'left', clickCount: i });
        await send('Input.dispatchMouseEvent', { type: 'mouseReleased', x, y, button: 'left', clickCount: i });
      }
      await sleep(150);
    },
    async type(text) { await send('Input.insertText', { text }); await sleep(80); },
    async key(name) {
      const codes = { Enter: 13, Escape: 27, Tab: 9 };
      for (const type of ['keyDown', 'keyUp']) {
        await send('Input.dispatchKeyEvent', { type, key: name, code: name, windowsVirtualKeyCode: codes[name], nativeVirtualKeyCode: codes[name] });
      }
      await sleep(150);
    },
    /** 组合键：modifiers Alt=1 Ctrl=2 Meta=4 Shift=8 */
    async combo(key, code, vk, modifiers) {
      for (const type of ['keyDown', 'keyUp']) {
        await send('Input.dispatchKeyEvent', { type, key, code, windowsVirtualKeyCode: vk, nativeVirtualKeyCode: vk, modifiers });
      }
      await sleep(300);
    },
    async shot(path) {
      const r = await send('Page.captureScreenshot', { format: 'png' });
      fs.writeFileSync(path, Buffer.from(r.data, 'base64'));
    },
    async waitFor(expr, ms = 5000) {
      const end = Date.now() + ms;
      while (Date.now() < end) {
        if (await page.eval(expr)) return true;
        await sleep(100);
      }
      return false;
    },
    close() { ws.close(); },
  };
  return page;
}

export const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

let failed = 0;
export function check(name, ok, detail = '') {
  if (!ok) failed++;
  console.log(`${ok ? 'PASS' : 'FAIL'} ${name}${detail ? ` — ${detail}` : ''}`);
}
export const failures = () => failed;

/** 以 / 结尾的按整个网址匹配，否则按包含匹配 */
function matches(url, part) {
  return part.endsWith('/') ? url === part : url.includes(part);
}
