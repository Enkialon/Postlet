import { reactive } from 'vue';
import {
  detectInitialLocale,
  persistLocale,
  type AppLocale,
  i18n,
} from '../i18n';

const WIDTH_KEY = 'postlet.panelWidths';

export interface PanelWidths {
  nav: number;
  list: number;
}

const preferences = reactive({
  locale: detectInitialLocale() as AppLocale,
  panelWidths: loadWidths(),
});

const LIST_MIN = 220;
const LIST_MAX = 520;

function clampList(n: number): number {
  return Math.min(LIST_MAX, Math.max(LIST_MIN, n));
}

function loadWidths(): PanelWidths {
  try {
    const raw = localStorage.getItem(WIDTH_KEY);
    if (!raw) return { nav: 160, list: 300 };
    const parsed = JSON.parse(raw) as PanelWidths;
    return {
      nav: typeof parsed.nav === 'number' ? parsed.nav : 160,
      list:
        typeof parsed.list === 'number' ? clampList(parsed.list) : 300,
    };
  } catch {
    return { nav: 160, list: 300 };
  }
}

export function usePreferences() {
  function setLocale(locale: AppLocale): void {
    preferences.locale = locale;
    persistLocale(locale);
    i18n.global.locale.value = locale;
  }

  function setPanelWidths(widths: Partial<PanelWidths>): void {
    if (widths.nav != null) preferences.panelWidths.nav = widths.nav;
    if (widths.list != null) {
      preferences.panelWidths.list = clampList(widths.list);
    }
    localStorage.setItem(WIDTH_KEY, JSON.stringify(preferences.panelWidths));
  }

  return {
    preferences,
    setLocale,
    setPanelWidths,
  };
}
