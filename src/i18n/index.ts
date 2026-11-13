import { createI18n } from 'vue-i18n';
import { zhCN } from './messages/zh-CN';
import { enUS } from './messages/en-US';

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

export const i18n = createI18n({
  legacy: false,
  locale: detectInitialLocale(),
  fallbackLocale: 'zh-CN',
  messages: {
    'zh-CN': zhCN,
    'en-US': enUS,
  },
});

/** 动态文案辅助：显式国际化 */
export function tDynamic(
  key: string,
  params?: Record<string, unknown>,
): string {
  return String(i18n.global.t(key, params ?? {}));
}
