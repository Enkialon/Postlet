import { computed, reactive } from 'vue';
import type { AccountId, MessageId, SessionId } from '../domain/types';

export type AppPage =
  | 'welcome'
  | 'conversations'
  | 'contacts'
  | 'groups'
  | 'settings';

export type SessionKindFilter = 'all' | 'private' | 'group';

/** `all` = 列表不按账号过滤；具体 id = 只查该账号 */
export type AccountFilter = AccountId | 'all';

const LAST_ACCOUNT_KEY = 'postlet.lastAccountId';
const ACCOUNT_FILTER_KEY = 'postlet.accountFilter';

/** 仅保存可恢复本地 ID，不含正文 / Token */
export interface NavigationState {
  /** 列表过滤：全部或单一账号 */
  accountFilter: AccountFilter;
  /** 当前读写上下文账号（会话/联系人归属；回复与发信跟这个走） */
  accountId: AccountId | null;
  page: AppPage;
  sessionId: SessionId | null;
  messageId: MessageId | null;
  sessionKind: SessionKindFilter;
  /** 账户切换时递增，丢弃迟到响应 */
  sessionVersion: number;
  narrowShowList: boolean;
  detailOpen: boolean;
}

const state = reactive<NavigationState>({
  accountFilter: 'all',
  accountId: null,
  page: 'welcome',
  sessionId: null,
  messageId: null,
  sessionKind: 'all',
  sessionVersion: 0,
  narrowShowList: true,
  detailOpen: false,
});

export function useNavigation() {
  const accountFilter = computed(() => state.accountFilter);
  const accountId = computed(() => state.accountId);
  const page = computed(() => state.page);
  const sessionId = computed(() => state.sessionId);
  const messageId = computed(() => state.messageId);
  const sessionKind = computed(() => state.sessionKind);
  const sessionVersion = computed(() => state.sessionVersion);
  const narrowShowList = computed(() => state.narrowShowList);
  const detailOpen = computed(() => state.detailOpen);

  /** 列表查询用的 account_id：全部 → null */
  const listAccountId = computed((): AccountId | null =>
    state.accountFilter === 'all' ? null : state.accountFilter,
  );

  function bumpSession(): number {
    state.sessionVersion += 1;
    return state.sessionVersion;
  }

  function setAccountFilter(filter: AccountFilter): void {
    state.accountFilter = filter;
    localStorage.setItem(ACCOUNT_FILTER_KEY, filter);
    if (filter !== 'all') {
      state.accountId = filter;
      localStorage.setItem(LAST_ACCOUNT_KEY, filter);
    }
  }

  function setAccount(id: AccountId | null): void {
    state.accountId = id;
    if (id) localStorage.setItem(LAST_ACCOUNT_KEY, id);
    else localStorage.removeItem(LAST_ACCOUNT_KEY);
  }

  function readLastAccountId(): AccountId | null {
    return localStorage.getItem(LAST_ACCOUNT_KEY);
  }

  function readAccountFilter(): AccountFilter {
    const raw = localStorage.getItem(ACCOUNT_FILTER_KEY);
    if (raw === 'all') return 'all';
    if (raw && raw !== 'all') return raw;
    return 'all';
  }

  function goWelcome(): void {
    state.page = 'welcome';
    state.sessionId = null;
    state.messageId = null;
    state.detailOpen = false;
  }

  function goPage(pageName: Exclude<AppPage, 'welcome'>): void {
    state.page = pageName;
    if (pageName !== 'conversations') {
      state.narrowShowList = true;
    }
  }

  function selectSession(id: SessionId | null): void {
    state.sessionId = id;
    state.messageId = null;
    state.detailOpen = false;
    state.narrowShowList = id == null;
    if (id) state.page = 'conversations';
  }

  function selectMessage(id: MessageId | null): void {
    state.messageId = id;
  }

  function setSessionKind(kind: SessionKindFilter): void {
    state.sessionKind = kind;
  }

  function setNarrowShowList(show: boolean): void {
    state.narrowShowList = show;
  }

  function setDetailOpen(open: boolean): void {
    state.detailOpen = open;
  }

  function clearReading(): void {
    state.sessionId = null;
    state.messageId = null;
    state.detailOpen = false;
  }

  return {
    state,
    accountFilter,
    accountId,
    listAccountId,
    page,
    sessionId,
    messageId,
    sessionKind,
    sessionVersion,
    narrowShowList,
    detailOpen,
    bumpSession,
    setAccountFilter,
    setAccount,
    readLastAccountId,
    readAccountFilter,
    goWelcome,
    goPage,
    selectSession,
    selectMessage,
    setSessionKind,
    setNarrowShowList,
    setDetailOpen,
    clearReading,
  };
}

export type NavigationApi = ReturnType<typeof useNavigation>;
