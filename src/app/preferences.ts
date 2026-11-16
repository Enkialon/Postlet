import { reactive } from 'vue';
import {
  detectInitialLocale,
  persistLocale,
  setI18nLocale,
  type AppLocale,
} from '../i18n';

const WIDTH_KEY = 'postlet.panelWidths';
const SHOW_DELETED_KEY = 'postlet.showDeletedInThread';

export interface PanelWidths {
  nav: number;
  list: number;
  /** 邮件阅读侧栏宽度 */
  mail: number;
  /** 会话详情侧栏宽度 */
  detail: number;
  /** 快捷写信高度 */
  compose: number;
}

const DEFAULT_WIDTHS: PanelWidths = {
  nav: 160,
  list: 300,
  mail: 560,
  detail: 280,
  compose: 330,
};

const LIST_MIN = 200;
const LIST_MAX = 520;
const MAIL_MIN = 320;
const MAIL_MAX = 1350;
const DETAIL_MIN = 200;
const DETAIL_MAX = 780;
const COMPOSE_MIN = 176;
const COMPOSE_MAX = 720;

function loadShowDeletedInThread(): boolean {
  try {
    return localStorage.getItem(SHOW_DELETED_KEY) === '1';
  } catch {
    return false;
  }
}

const preferences = reactive({
  locale: detectInitialLocale() as AppLocale,
  panelWidths: loadWidths(),
  /** 聊天线程是否显示已删除邮件（默认隐藏；后端仍返回，由前端过滤） */
  showDeletedInThread: loadShowDeletedInThread(),
});

function clamp(n: number, min: number, max: number): number {
  return Math.min(max, Math.max(min, n));
}

function loadWidths(): PanelWidths {
  try {
    const raw = localStorage.getItem(WIDTH_KEY);
    if (!raw) return { ...DEFAULT_WIDTHS };
    const parsed = JSON.parse(raw) as Partial<PanelWidths>;
    return {
      nav: typeof parsed.nav === 'number' ? parsed.nav : DEFAULT_WIDTHS.nav,
      list:
        typeof parsed.list === 'number'
          ? clamp(parsed.list, LIST_MIN, LIST_MAX)
          : DEFAULT_WIDTHS.list,
      mail:
        typeof parsed.mail === 'number'
          ? clamp(parsed.mail, MAIL_MIN, MAIL_MAX)
          : DEFAULT_WIDTHS.mail,
      detail:
        typeof parsed.detail === 'number'
          ? clamp(parsed.detail, DETAIL_MIN, DETAIL_MAX)
          : DEFAULT_WIDTHS.detail,
      compose:
        typeof parsed.compose === 'number'
          ? clamp(parsed.compose, COMPOSE_MIN, COMPOSE_MAX)
          : DEFAULT_WIDTHS.compose,
    };
  } catch {
    return { ...DEFAULT_WIDTHS };
  }
}

export function usePreferences() {
  function setLocale(locale: AppLocale): void {
    preferences.locale = locale;
    persistLocale(locale);
    setI18nLocale(locale);
  }

  function setPanelWidths(widths: Partial<PanelWidths>): void {
    if (widths.nav != null) preferences.panelWidths.nav = widths.nav;
    if (widths.list != null) {
      preferences.panelWidths.list = clamp(widths.list, LIST_MIN, LIST_MAX);
    }
    if (widths.mail != null) {
      preferences.panelWidths.mail = clamp(widths.mail, MAIL_MIN, MAIL_MAX);
    }
    if (widths.detail != null) {
      preferences.panelWidths.detail = clamp(
        widths.detail,
        DETAIL_MIN,
        DETAIL_MAX,
      );
    }
    if (widths.compose != null) {
      preferences.panelWidths.compose = clamp(
        widths.compose,
        COMPOSE_MIN,
        COMPOSE_MAX,
      );
    }
    localStorage.setItem(WIDTH_KEY, JSON.stringify(preferences.panelWidths));
  }

  function setShowDeletedInThread(next: boolean): void {
    preferences.showDeletedInThread = next;
    try {
      localStorage.setItem(SHOW_DELETED_KEY, next ? '1' : '0');
    } catch {
      /* ignore */
    }
  }

  return {
    preferences,
    setLocale,
    setPanelWidths,
    setShowDeletedInThread,
    limits: {
      list: { min: LIST_MIN, max: LIST_MAX },
      mail: { min: MAIL_MIN, max: MAIL_MAX },
      detail: { min: DETAIL_MIN, max: DETAIL_MAX },
      compose: { min: COMPOSE_MIN, max: COMPOSE_MAX },
    },
  };
}
