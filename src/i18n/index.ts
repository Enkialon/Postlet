import { createI18n, type PathValue } from 'vue-i18n';
import zhCN from './locales/zh-CN.json';
import enUS from './locales/en-US.json';

export type AppLocale = 'zh-CN' | 'en-US';

const STORAGE_KEY = 'postlet.locale';

export function detectInitialLocale(): AppLocale {
  const saved = localStorage.getItem(STORAGE_KEY);
  if (saved === 'zh-CN' || saved === 'en-US') return saved;
  const sys = navigator.language.toLowerCase();
  if (sys.startsWith('en')) return 'en-US';
  return 'zh-CN';
}

export function persistLocale(locale: AppLocale): void {
  localStorage.setItem(STORAGE_KEY, locale);
}

/**
 * 中文原文作 Key，可能含 `.` 等字符；用扁平查找，避免 vue-i18n 按路径拆分。
 */
function flatMessageResolver(obj: unknown, path: string): PathValue {
  if (obj != null && typeof obj === 'object' && path in (obj as object)) {
    return (obj as Record<string, PathValue>)[path] ?? null;
  }
  return null;
}

export const i18n = createI18n({
  legacy: false,
  locale: detectInitialLocale(),
  fallbackLocale: 'zh-CN',
  messages: {
    'zh-CN': zhCN,
    'en-US': enUS,
  },
  messageResolver: flatMessageResolver,
});

export function setI18nLocale(locale: AppLocale): void {
  i18n.global.locale.value = locale;
}

/** 动态文案辅助：显式国际化 */
export function tDynamic(
  key: string,
  params?: Record<string, unknown>,
): string {
  return String(i18n.global.t(key, params ?? {}));
}
