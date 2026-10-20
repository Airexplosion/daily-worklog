// @ts-check
// 框选页的按键 → 动作（纯函数）。输入法选字、正在打字时不抢键

/**
 * @param {{key: string, ctrlKey: boolean, shiftKey: boolean, isComposing: boolean, keyCode: number}} ev
 * @param {{hasRect: boolean, editingText: boolean}} ctx
 * @returns {'primary'|'record'|'copy'|'save'|'undo'|'cancel'|null}
 */
export function keyAction(ev, ctx) {
  if (ev.isComposing || ev.keyCode === 229 || ctx.editingText) return null;
  const key = ev.key.length === 1 ? ev.key.toLowerCase() : ev.key;
  if (key === 'Escape') return 'cancel';
  if (!ctx.hasRect) return null;
  if (ev.ctrlKey) {
    if (key === 'c') return 'copy';
    if (key === 's') return 'save';
    if (key === 'z') return 'undo';
    return null;
  }
  if (key === 'Enter') return 'primary';
  if (key === 'Tab') return 'record';
  return null;
}
