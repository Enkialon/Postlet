import { computed, reactive } from 'vue';
import type { AccountId, ConversationId, MessageId } from '../domain/types';

export type AppPage =
  | 'welcome'
  | 'conversations'
  | 'contacts'
  | 'groups'
  | 'settings';

export type ConversationKindFilter = 'all' | 'contact' | 'group';

const LAST_ACCOUNT_KEY = 'postlet.lastAccountId';

/** 仅保存可恢复本地 ID，不含正文 / Token */
export interface NavigationState {
  accountId: AccountId | null;
  page: AppPage;
  conversationId: ConversationId | null;
  messageId: MessageId | null;
  conversationKind: ConversationKindFilter;
  /** 账户切换时递增，丢弃迟到响应 */
  sessionVersion: number;
  narrowShowList: boolean;
  detailOpen: boolean;
}

const state = reactive<NavigationState>({
  accountId: null,
  page: 'welcome',
  conversationId: null,
  messageId: null,
  conversationKind: 'all',
  sessionVersion: 0,
  narrowShowList: true,
  detailOpen: false,
});

export function useNavigation() {
  const accountId = computed(() => state.accountId);
  const page = computed(() => state.page);
  const conversationId = computed(() => state.conversationId);
  const messageId = computed(() => state.messageId);
  const conversationKind = computed(() => state.conversationKind);
  const sessionVersion = computed(() => state.sessionVersion);
  const narrowShowList = computed(() => state.narrowShowList);
  const detailOpen = computed(() => state.detailOpen);

  function bumpSession(): number {
    state.sessionVersion += 1;
    return state.sessionVersion;
  }

  function setAccount(id: AccountId | null): void {
    state.accountId = id;
    if (id) localStorage.setItem(LAST_ACCOUNT_KEY, id);
    else localStorage.removeItem(LAST_ACCOUNT_KEY);
  }

  function readLastAccountId(): AccountId | null {
    return localStorage.getItem(LAST_ACCOUNT_KEY);
  }

  function goWelcome(): void {
    state.page = 'welcome';
    state.conversationId = null;
    state.messageId = null;
    state.detailOpen = false;
  }

  function goPage(pageName: Exclude<AppPage, 'welcome'>): void {
    state.page = pageName;
    if (pageName !== 'conversations') {
      state.narrowShowList = true;
    }
  }

  function selectConversation(id: ConversationId | null): void {
    state.conversationId = id;
    state.messageId = null;
    state.detailOpen = false;
    state.narrowShowList = id == null;
    if (id) state.page = 'conversations';
  }

  function selectMessage(id: MessageId | null): void {
    state.messageId = id;
  }

  function setConversationKind(kind: ConversationKindFilter): void {
    state.conversationKind = kind;
  }

  function setNarrowShowList(show: boolean): void {
    state.narrowShowList = show;
  }

  function setDetailOpen(open: boolean): void {
    state.detailOpen = open;
  }

  function clearReading(): void {
    state.conversationId = null;
    state.messageId = null;
    state.detailOpen = false;
  }

  return {
    state,
    accountId,
    page,
    conversationId,
    messageId,
    conversationKind,
    sessionVersion,
    narrowShowList,
    detailOpen,
    bumpSession,
    setAccount,
    readLastAccountId,
    goWelcome,
    goPage,
    selectConversation,
    selectMessage,
    setConversationKind,
    setNarrowShowList,
    setDetailOpen,
    clearReading,
  };
}

export type NavigationApi = ReturnType<typeof useNavigation>;
