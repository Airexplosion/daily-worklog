// @ts-check
// Fluent 风格的线条图标（20×20 视框，描边 1.5）

/** @param {string} body */
const svg = (body) =>
  `<svg class="ico" viewBox="0 0 20 20" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">${body}</svg>`;

export const icons = {
  today: svg('<rect x="3" y="4" width="14" height="13" rx="2"/><path d="M3 8h14M7 2.5v3M13 2.5v3"/><circle cx="10" cy="12.5" r="1.4" fill="currentColor" stroke="none"/>'),
  history: svg('<path d="M3.5 10a6.5 6.5 0 1 0 2-4.7"/><path d="M3 3.5v3h3"/><path d="M10 6.5V10l2.5 1.8"/>'),
  settings: svg('<circle cx="10" cy="10" r="2.5"/><path d="M10 2.5v2M10 15.5v2M2.5 10h2M15.5 10h2M4.7 4.7l1.4 1.4M13.9 13.9l1.4 1.4M4.7 15.3l1.4-1.4M13.9 6.1l1.4-1.4"/>'),
  camera: svg('<path d="M3 7.5A1.5 1.5 0 0 1 4.5 6h2l1.2-2h4.6l1.2 2h2A1.5 1.5 0 0 1 17 7.5v7a1.5 1.5 0 0 1-1.5 1.5h-11A1.5 1.5 0 0 1 3 14.5z"/><circle cx="10" cy="10.8" r="2.8"/>'),
  add: svg('<path d="M10 4v12M4 10h12"/>'),
  delete: svg('<path d="M4 6h12M8 6V4.5h4V6M6 6l.7 10h6.6L14 6"/>'),
  edit: svg('<path d="M12.5 4.5l3 3L8 15H5v-3z"/>'),
  copy: svg('<rect x="7" y="7" width="9" height="10" rx="1.5"/><path d="M13 7V4.5A1.5 1.5 0 0 0 11.5 3h-6A1.5 1.5 0 0 0 4 4.5v8A1.5 1.5 0 0 0 5.5 14H7"/>'),
  refresh: svg('<path d="M16 10a6 6 0 1 1-1.8-4.3"/><path d="M16 3.5V7h-3.5"/>'),
  close: svg('<path d="M5 5l10 10M15 5L5 15"/>'),
  check: svg('<path d="M4.5 10.5l3.5 3.5 7.5-8"/>'),
  warning: svg('<path d="M10 3l7.5 13h-15z"/><path d="M10 8v3.5"/><circle cx="10" cy="14" r=".6" fill="currentColor"/>'),
  info: svg('<circle cx="10" cy="10" r="7"/><path d="M10 9v4.5"/><circle cx="10" cy="6.5" r=".6" fill="currentColor"/>'),
  back: svg('<path d="M11.5 5L6.5 10l5 5"/>'),
  chevron: svg('<path d="M6 8l4 4 4-4"/>'),
  sparkle: svg('<path d="M10 3l1.6 4.4L16 9l-4.4 1.6L10 15l-1.6-4.4L4 9l4.4-1.6z"/>'),
  folder: svg('<path d="M3 6a1.5 1.5 0 0 1 1.5-1.5h3l1.5 1.5h6.5A1.5 1.5 0 0 1 17 7.5v7a1.5 1.5 0 0 1-1.5 1.5h-11A1.5 1.5 0 0 1 3 14.5z"/>'),
  bellOff: svg('<path d="M6 8a4 4 0 0 1 7-2.6M14 9v2.5l1.5 2.5H6M4.5 14L5 13V9.5M8.5 16a1.5 1.5 0 0 0 3 0M3 3l14 14"/>'),
};
