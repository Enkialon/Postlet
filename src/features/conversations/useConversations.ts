import { inject, reactive, ref, watch } from 'vue';
import type { ConversationListItem } from '../../domain/dto';
import type { ConversationKind } from '../../domain/types';
import { portsKey, navigationKey } from '../../app/provide';
import {
  createEmptyPageState,
  type PageState,
} from '../../shared/state/pageState';
import { asAppError } from '../../shared/errors/asAppError';

const PAGE_SIZE = 50;

const listState = reactive<PageState<ConversationListItem>>(
  createEmptyPageState(),
);
const selected = ref<ConversationListItem | null>(null);
const staleWarning = ref(false);
let watchBound = false;

export function useConversations() {
  const ports = inject(portsKey)!;
  const nav = inject(navigationKey)!;

  function kindParam(): ConversationKind | null {
    const k = nav.conversationKind.value;
    if (k === 'all') return null;
    return k;
  }

  function listQueryAccountId() {
    return nav.listAccountId.value;
  }

  async function loadInitial(): Promise<void> {
    const filter = nav.accountFilter.value;
    const version = ++listState.requestVersion;
    const session = nav.sessionVersion.value;
    listState.loading = 'initial';
    listState.error = null;
    staleWarning.value = false;
    try {
      const page = await ports.conversation.listConversations({
        account_id: listQueryAccountId(),
        cursor: null,
        limit: PAGE_SIZE,
        kind: kindParam(),
      });
      if (
        version !== listState.requestVersion ||
        session !== nav.sessionVersion.value ||
        filter !== nav.accountFilter.value
      ) {
        return;
      }
      if (page.has_more && !page.next_cursor) {
        listState.error = {
          code: 'internal',
          message: '分页契约错误：has_more 但无游标',
          retryable: false,
        };
        listState.items = page.items;
        listState.hasMore = false;
        listState.nextCursor = null;
        return;
      }
      listState.items = page.items;
      listState.nextCursor = page.next_cursor;
      listState.hasMore = page.has_more;
      await restoreSelection();
    } catch (e) {
      if (version !== listState.requestVersion) return;
      listState.error = asAppError(e);
    } finally {
      if (version === listState.requestVersion) listState.loading = 'idle';
    }
  }

  async function loadMore(): Promise<void> {
    if (!listState.hasMore || !listState.nextCursor) return;
    if (listState.loading !== 'idle') return;
    const filter = nav.accountFilter.value;
    const version = ++listState.requestVersion;
    const session = nav.sessionVersion.value;
    listState.loading = 'more';
    try {
      const page = await ports.conversation.listConversations({
        account_id: listQueryAccountId(),
        cursor: listState.nextCursor,
        limit: PAGE_SIZE,
        kind: kindParam(),
      });
      if (
        version !== listState.requestVersion ||
        session !== nav.sessionVersion.value ||
        filter !== nav.accountFilter.value
      ) {
        return;
      }
      listState.items = [...listState.items, ...page.items];
      listState.nextCursor = page.next_cursor;
      listState.hasMore = page.has_more;
      if (page.has_more && !page.next_cursor) {
        listState.hasMore = false;
        listState.error = {
          code: 'internal',
          message: '分页契约错误：has_more 但无游标',
          retryable: false,
        };
      }
    } catch (e) {
      if (version !== listState.requestVersion) return;
      listState.error = asAppError(e);
    } finally {
      if (version === listState.requestVersion) listState.loading = 'idle';
    }
  }

  async function refreshSoft(): Promise<void> {
    const filter = nav.accountFilter.value;
    const version = ++listState.requestVersion;
    const session = nav.sessionVersion.value;
    listState.loading = 'refresh';
    try {
      const page = await ports.conversation.listConversations({
        account_id: listQueryAccountId(),
        cursor: null,
        limit: PAGE_SIZE,
        kind: kindParam(),
      });
      if (
        version !== listState.requestVersion ||
        session !== nav.sessionVersion.value ||
        filter !== nav.accountFilter.value
      ) {
        return;
      }
      listState.items = page.items;
      listState.nextCursor = page.next_cursor;
      listState.hasMore = page.has_more;
      listState.error = null;
      staleWarning.value = false;
      await restoreSelection();
    } catch (e) {
      if (version !== listState.requestVersion) return;
      staleWarning.value = true;
      void e;
    } finally {
      if (version === listState.requestVersion) listState.loading = 'idle';
    }
  }

  async function restoreSelection(): Promise<void> {
    const id = nav.conversationId.value;
    const accountId = nav.accountId.value;
    if (!id || !accountId) {
      selected.value = null;
      return;
    }
    const found = listState.items.find(
      (i) =>
        i.conversation.id === id && i.conversation.account_id === accountId,
    );
    if (found) {
      selected.value = found;
      return;
    }
    try {
      selected.value = await ports.conversation.getConversation(accountId, id);
    } catch {
      selected.value = null;
      nav.selectConversation(null);
    }
  }

  async function openConversation(
    accountId: string,
    conversationId: string,
  ): Promise<void> {
    nav.setAccount(accountId);
    nav.selectConversation(conversationId);
    try {
      selected.value = await ports.conversation.getConversation(
        accountId,
        conversationId,
      );
    } catch (e) {
      selected.value = null;
      listState.error = asAppError(e);
    }
  }

  function invalidate(): void {
    void loadInitial();
  }

  if (!watchBound) {
    watchBound = true;
    watch(
      () => [
        nav.accountFilter.value,
        nav.conversationKind.value,
        nav.sessionVersion.value,
      ],
      () => {
        selected.value = null;
        Object.assign(listState, createEmptyPageState<ConversationListItem>());
        void loadInitial();
      },
    );
  }

  return {
    listState,
    selected,
    staleWarning,
    loadInitial,
    loadMore,
    refreshSoft,
    openConversation,
    invalidate,
  };
}
