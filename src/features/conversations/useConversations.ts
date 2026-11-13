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

  async function loadInitial(): Promise<void> {
    const accountId = nav.accountId.value;
    if (!accountId) return;
    const version = ++listState.requestVersion;
    const session = nav.sessionVersion.value;
    listState.loading = 'initial';
    listState.error = null;
    staleWarning.value = false;
    try {
      const page = await ports.conversation.listConversations({
        account_id: accountId,
        cursor: null,
        limit: PAGE_SIZE,
        kind: kindParam(),
      });
      if (
        version !== listState.requestVersion ||
        session !== nav.sessionVersion.value ||
        accountId !== nav.accountId.value
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
      restoreSelection(accountId);
    } catch (e) {
      if (version !== listState.requestVersion) return;
      listState.error = asAppError(e);
    } finally {
      if (version === listState.requestVersion) listState.loading = 'idle';
    }
  }

  async function loadMore(): Promise<void> {
    const accountId = nav.accountId.value;
    if (!accountId || !listState.hasMore || listState.loading !== 'idle') return;
    if (listState.hasMore && !listState.nextCursor) return;
    const version = listState.requestVersion;
    const session = nav.sessionVersion.value;
    listState.loading = 'more';
    try {
      const page = await ports.conversation.listConversations({
        account_id: accountId,
        cursor: listState.nextCursor,
        limit: PAGE_SIZE,
        kind: kindParam(),
      });
      if (
        version !== listState.requestVersion ||
        session !== nav.sessionVersion.value
      ) {
        return;
      }
      const byId = new Map(listState.items.map((i) => [i.conversation.id, i]));
      for (const item of page.items) byId.set(item.conversation.id, item);
      listState.items = [...byId.values()];
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
    const accountId = nav.accountId.value;
    if (!accountId) return;
    const version = ++listState.requestVersion;
    const session = nav.sessionVersion.value;
    listState.loading = 'refresh';
    try {
      const page = await ports.conversation.listConversations({
        account_id: accountId,
        cursor: null,
        limit: PAGE_SIZE,
        kind: kindParam(),
      });
      if (
        version !== listState.requestVersion ||
        session !== nav.sessionVersion.value
      ) {
        return;
      }
      listState.items = page.items;
      listState.nextCursor = page.next_cursor;
      listState.hasMore = page.has_more;
      listState.error = null;
      staleWarning.value = false;
      restoreSelection(accountId);
    } catch (e) {
      if (version !== listState.requestVersion) return;
      staleWarning.value = true;
      void e;
    } finally {
      if (version === listState.requestVersion) listState.loading = 'idle';
    }
  }

  async function restoreSelection(accountId: string): Promise<void> {
    const id = nav.conversationId.value;
    if (!id) {
      selected.value = null;
      return;
    }
    const found = listState.items.find((i) => i.conversation.id === id);
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

  async function openConversation(id: string): Promise<void> {
    const accountId = nav.accountId.value;
    if (!accountId) return;
    nav.selectConversation(id);
    try {
      selected.value = await ports.conversation.getConversation(accountId, id);
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
        nav.accountId.value,
        nav.conversationKind.value,
        nav.sessionVersion.value,
      ],
      () => {
        selected.value = null;
        Object.assign(listState, createEmptyPageState<ConversationListItem>());
        if (nav.accountId.value) void loadInitial();
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
