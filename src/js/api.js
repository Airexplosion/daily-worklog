// @ts-check
// 后端命令的封装与数据类型

const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

/**
 * @typedef {'recognizing' | 'done' | 'pending'} ItemStatus
 * @typedef {{id: string, createdAt: string, text: string, note: string, source: 'ai' | 'manual',
 *   edited: boolean, tags: string[], confidence: number | null, shots: string[], status: ItemStatus,
 *   error?: string}} Item
 * @typedef {{text: string, generatedAt: string, editedAt: string | null, previousText: string | null}} Summary
 * @typedef {{date: string, label: string, isToday: boolean, items: Item[], summary: Summary | null,
 *   stale: boolean, muted: boolean, remindTime: string, hotkey: string}} DayView
 * @typedef {{name: string, byAi: boolean, isNew: boolean}} TagView
 */

export const api = {
  /** @returns {Promise<string>} */
  today: () => invoke('today'),
  /** @param {string} date @returns {Promise<DayView>} */
  getDay: (date) => invoke('get_day', { date }),
  /** @returns {Promise<TagView[]>} */
  getTags: () => invoke('get_tags'),
  /** @param {string} date @param {string} text @returns {Promise<Item>} */
  addItem: (date, text) => invoke('add_item', { date, text }),
  /** @param {string} id @param {{text?: string, tags?: string[]}} patch */
  updateItem: (id, patch) => invoke('update_item', { id, text: patch.text ?? null, tags: patch.tags ?? null }),
  /** @param {string} id @returns {Promise<string>} */
  deleteItem: (id) => invoke('delete_item', { id }),
  /** @param {string} token */
  undoDelete: (token) => invoke('undo_delete', { token }),
  /** @param {string} id */
  retryItem: (id) => invoke('retry_item', { id }),
  /** @param {string} date */
  retryAll: (date) => invoke('retry_all', { date }),
  /** @param {string} name */
  confirmTag: (name) => invoke('confirm_tag', { name }),
  /** @param {string} from @param {string} to */
  renameTag: (from, to) => invoke('rename_tag', { from, to }),
  muteToday: () => invoke('mute_today'),
  captureFromMain: () => invoke('capture_from_main'),
  /** @param {string} rel shots/xxx.png @returns {string} */
  shotUrl: (rel) => `http://shot.localhost/${rel.split('/').pop()}`,
  listen,
};
