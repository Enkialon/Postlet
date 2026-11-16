import { computed, inject, reactive, ref, watch } from 'vue';
import type { SessionListItem } from '../../domain/dto';
import type { SessionKind } from '../../domain/types';
import { portsKey, navigationKey } from '../../app/provide';
import {
  createEmptyPageState,
  type PageState,
} from '../../shared/state/pageState';
import { asAppError } from '../../shared/errors/asAppError';
import {
  appendSessionPage,
  applySessionSnapshot,
  sessionListKey,
  itemsFromDisplayOrder,
} from './conversationListOrder';

const PAGE_SIZE = 50;

const listState = reactive<PageState<SessionListItem>>(createEmptyPageState());
const selected = ref<SessionListItem | null>(null);
const staleWarning = ref(false);

/** 最新会话数据（稳定 key → 对象，字段原地更新） */
const sessionByKey = reactive(new Map<string, SessionListItem>());
const canonicalOrder = ref<string[]>([]);
const displayOrder = ref<string[]>([]);
/** 用户是否在会话列表顶部（由列表组件上报） */
const listAtTop = ref(true);
const pendingActivityKeys = ref<string[]>([]);

const newActivityCount = computed(() => pendingActivityKeys.value.length);
const showNewActivityHint = computed(
  () => !listAtTop.value && pendingActivityKeys.value.length > 0,
);

let watchBound = false;

function syncListItemsFromDisplay(): void {
  listState.items = itemsFromDisplayOrder(sessionByKey, displayOrder.value);
}

function resetOrderState(): void {
  sessionByKey.clear();
  canonicalOrder.value = [];
  displayOrder.value = [];
  pendingActivityKeys.value = [];
  listAtTop.value = true;
}

function applyPageToState(
  pageItems: SessionListItem[],
  options: { mode: 'replace' | 'append'; forceApplyOrder: boolean },
): void {
  if (options.mode === 'append') {
    const next = appendSessionPage(
      new Map(sessionByKey),
      displayOrder.value,
      canonicalOrder.value,
      pageItems,
    );
    sessionByKey.clear();
    for (const [k, v] of next.byKey) sessionByKey.set(k, v);
    displayOrder.value = next.displayOrder;
    canonicalOrder.value = next.canonicalOrder;
    syncListItemsFromDisplay();
    return;
  }

  const result = applySessionSnapshot({
    snapshot: pageItems,
    previousByKey: new Map(sessionByKey),
    previousCanonicalOrder: canonicalOrder.value,
    previousDisplayOrder: displayOrder.value,
    previousPendingActivityKeys: pendingActivityKeys.value,
    atTop: listAtTop.value,
    forceApplyOrder: options.forceApplyOrder,
  });

  sessionByKey.clear();
  for (const [k, v] of result.byKey) sessionByKey.set(k, v);
  canonicalOrder.value = result.canonicalOrder;
  displayOrder.value = result.displayOrder;
  pendingActivityKeys.value = result.pendingActivityKeys;
  syncListItemsFromDisplay();
}

function applyCanonicalOrderNow(): void {
  displayOrder.value = [...canonicalOrder.value];
  const keep = new Set(canonicalOrder.value);
  for (const key of [...sessionByKey.keys()]) {
    if (!keep.has(key)) sessionByKey.delete(key);
  }
  pendingActivityKeys.value = [];
  syncListItemsFromDisplay();
}

export function useConversations() {
  const ports = inject(portsKey)!;
  const nav = inject(navigationKey)!;

  function kindParam(): SessionKind | null {
    const k = nav.sessionKind.value;
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
      const page = await ports.session.listSessions({
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
        applyPageToState(page.items, {
          mode: 'replace',
          forceApplyOrder: true,
        });
        listState.hasMore = false;
        listState.nextCursor = null;
        return;
      }
      applyPageToState(page.items, { mode: 'replace', forceApplyOrder: true });
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
      const page = await ports.session.listSessions({
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
      applyPageToState(page.items, { mode: 'append', forceApplyOrder: false });
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
      const page = await ports.session.listSessions({
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
      applyPageToState(page.items, {
        mode: 'replace',
        forceApplyOrder: false,
      });
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
    const id = nav.sessionId.value;
    const accountId = nav.accountId.value;
    if (!id || !accountId) {
      selected.value = null;
      return;
    }
    const key = `${accountId}:${id}`;
    const found = sessionByKey.get(key);
    if (found) {
      selected.value = found;
      return;
    }
    const inList = listState.items.find(
      (i) => i.session.id === id && i.session.account_id === accountId,
    );
    if (inList) {
      selected.value = inList;
      return;
    }
    if (
      selected.value?.session.id === id &&
      selected.value.session.account_id === accountId
    ) {
      return;
    }
    selected.value = null;
  }

  async function openConversation(
    accountId: string,
    sessionId: string,
  ): Promise<void> {
    nav.setAccount(accountId);
    nav.selectSession(sessionId);

    const key = `${accountId}:${sessionId}`;
    const cached =
      sessionByKey.get(key) ??
      listState.items.find(
        (i) =>
          i.session.id === sessionId && i.session.account_id === accountId,
      ) ??
      null;
    if (cached) {
      selected.value = cached;
    }

    try {
      const item = await ports.session.getSession(accountId, sessionId);
      if (
        nav.sessionId.value !== sessionId ||
        nav.accountId.value !== accountId
      ) {
        return;
      }
      const itemKey = sessionListKey(item);
      const merged = sessionByKey.get(itemKey);
      if (merged) {
        Object.assign(merged.session, item.session);
        merged.display_name = item.display_name;
        merged.contact_display_name = item.contact_display_name;
        merged.group_name = item.group_name;
        if (item.last_message_preview) {
          if (merged.last_message_preview) {
            Object.assign(merged.last_message_preview, item.last_message_preview);
          } else {
            merged.last_message_preview = { ...item.last_message_preview };
          }
        }
        selected.value = merged;
      } else {
        selected.value = item;
      }
    } catch (e) {
      if (
        nav.sessionId.value !== sessionId ||
        nav.accountId.value !== accountId
      ) {
        return;
      }
      if (!cached) {
        selected.value = null;
        listState.error = asAppError(e);
      }
    }
  }

  function invalidate(): void {
    void loadInitial();
  }

  /** 本地隐藏：从列表移除；不删邮件。选中中则清空阅读。 */
  async function hideConversation(item: SessionListItem): Promise<void> {
    const accountId = item.session.account_id;
    const sessionId = item.session.id;
    const key = sessionListKey(item);
    try {
      await ports.session.hideSession(accountId, sessionId);
      sessionByKey.delete(key);
      canonicalOrder.value = canonicalOrder.value.filter((k) => k !== key);
      displayOrder.value = displayOrder.value.filter((k) => k !== key);
      pendingActivityKeys.value = pendingActivityKeys.value.filter(
        (k) => k !== key,
      );
      syncListItemsFromDisplay();
      if (
        nav.sessionId.value === sessionId &&
        nav.accountId.value === accountId
      ) {
        selected.value = null;
        nav.clearReading();
      }
    } catch (e) {
      listState.error = asAppError(e);
    }
  }

  function setListAtTop(next: boolean): void {
    const wasTop = listAtTop.value;
    listAtTop.value = next;
    if (!wasTop && next && pendingActivityKeys.value.length > 0) {
      applyCanonicalOrderNow();
    } else if (!wasTop && next) {
      if (
        displayOrder.value.length !== canonicalOrder.value.length ||
        displayOrder.value.some((k, i) => k !== canonicalOrder.value[i])
      ) {
        applyCanonicalOrderNow();
      }
    }
  }

  function jumpToLatestOrder(): void {
    listAtTop.value = true;
    applyCanonicalOrderNow();
  }

  if (!watchBound) {
    watchBound = true;
    watch(
      () => [
        nav.accountFilter.value,
        nav.sessionKind.value,
        nav.sessionVersion.value,
      ],
      () => {
        selected.value = null;
        nav.clearReading();
        listState.requestVersion += 1;
        listState.items = [];
        listState.nextCursor = null;
        listState.hasMore = false;
        listState.error = null;
        listState.loading = 'idle';
        resetOrderState();
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
    hideConversation,
    invalidate,
    listAtTop,
    setListAtTop,
    newActivityCount,
    showNewActivityHint,
    jumpToLatestOrder,
    canonicalOrder,
    displayOrder,
  };
}
