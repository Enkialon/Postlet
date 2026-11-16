import { computed, inject, reactive, ref, watch } from 'vue';
import type { ConversationListItem } from '../../domain/dto';
import type { ConversationKind } from '../../domain/types';
import { portsKey, navigationKey } from '../../app/provide';
import {
  createEmptyPageState,
  type PageState,
} from '../../shared/state/pageState';
import { asAppError } from '../../shared/errors/asAppError';
import {
  appendConversationPage,
  applyConversationSnapshot,
  conversationListKey,
  itemsFromDisplayOrder,
} from './conversationListOrder';

const PAGE_SIZE = 50;

const listState = reactive<PageState<ConversationListItem>>(
  createEmptyPageState(),
);
const selected = ref<ConversationListItem | null>(null);
const staleWarning = ref(false);

/** 最新会话数据（稳定 key → 对象，字段原地更新） */
const conversationByKey = reactive(new Map<string, ConversationListItem>());
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
  listState.items = itemsFromDisplayOrder(
    conversationByKey,
    displayOrder.value,
  );
}

function resetOrderState(): void {
  conversationByKey.clear();
  canonicalOrder.value = [];
  displayOrder.value = [];
  pendingActivityKeys.value = [];
  listAtTop.value = true;
}

function applyPageToState(
  pageItems: ConversationListItem[],
  options: { mode: 'replace' | 'append'; forceApplyOrder: boolean },
): void {
  if (options.mode === 'append') {
    const next = appendConversationPage(
      new Map(conversationByKey),
      displayOrder.value,
      canonicalOrder.value,
      pageItems,
    );
    conversationByKey.clear();
    for (const [k, v] of next.byKey) conversationByKey.set(k, v);
    displayOrder.value = next.displayOrder;
    canonicalOrder.value = next.canonicalOrder;
    syncListItemsFromDisplay();
    return;
  }

  const result = applyConversationSnapshot({
    snapshot: pageItems,
    previousByKey: new Map(conversationByKey),
    previousCanonicalOrder: canonicalOrder.value,
    previousDisplayOrder: displayOrder.value,
    previousPendingActivityKeys: pendingActivityKeys.value,
    atTop: listAtTop.value,
    forceApplyOrder: options.forceApplyOrder,
  });

  conversationByKey.clear();
  for (const [k, v] of result.byKey) conversationByKey.set(k, v);
  canonicalOrder.value = result.canonicalOrder;
  displayOrder.value = result.displayOrder;
  pendingActivityKeys.value = result.pendingActivityKeys;
  syncListItemsFromDisplay();
}

function applyCanonicalOrderNow(): void {
  displayOrder.value = [...canonicalOrder.value];
  // 回顶应用排序时丢掉仅存在于 loadMore 的多余项
  const keep = new Set(canonicalOrder.value);
  for (const key of [...conversationByKey.keys()]) {
    if (!keep.has(key)) conversationByKey.delete(key);
  }
  pendingActivityKeys.value = [];
  syncListItemsFromDisplay();
}

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
    const id = nav.conversationId.value;
    const accountId = nav.accountId.value;
    if (!id || !accountId) {
      selected.value = null;
      return;
    }
    const key = `${accountId}:${id}`;
    const found = conversationByKey.get(key);
    if (found) {
      selected.value = found;
      return;
    }
    const inList = listState.items.find(
      (i) =>
        i.conversation.id === id && i.conversation.account_id === accountId,
    );
    if (inList) {
      selected.value = inList;
      return;
    }
    // 仍指向当前选中则保留；勿再 IPC 拉回已不在筛选结果中的旧会话
    if (
      selected.value?.conversation.id === id &&
      selected.value.conversation.account_id === accountId
    ) {
      return;
    }
    selected.value = null;
  }

  async function openConversation(
    accountId: string,
    conversationId: string,
  ): Promise<void> {
    nav.setAccount(accountId);
    nav.selectConversation(conversationId);

    // 先用本地列表项立刻切换，避免干等 getConversation IPC
    const key = `${accountId}:${conversationId}`;
    const cached =
      conversationByKey.get(key) ??
      listState.items.find(
        (i) =>
          i.conversation.id === conversationId &&
          i.conversation.account_id === accountId,
      ) ??
      null;
    if (cached) {
      selected.value = cached;
    }

    try {
      const item = await ports.conversation.getConversation(
        accountId,
        conversationId,
      );
      // 用户已点到别的会话则丢弃迟到响应
      if (
        nav.conversationId.value !== conversationId ||
        nav.accountId.value !== accountId
      ) {
        return;
      }
      const itemKey = conversationListKey(item);
      const merged = conversationByKey.get(itemKey);
      if (merged) {
        Object.assign(merged.conversation, item.conversation);
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
        nav.conversationId.value !== conversationId ||
        nav.accountId.value !== accountId
      ) {
        return;
      }
      // 列表里已有缓存时保留选中，只记错误
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
  async function hideConversation(item: ConversationListItem): Promise<void> {
    const accountId = item.conversation.account_id;
    const conversationId = item.conversation.id;
    const key = conversationListKey(item);
    try {
      await ports.conversation.hideConversation(accountId, conversationId);
      conversationByKey.delete(key);
      canonicalOrder.value = canonicalOrder.value.filter((k) => k !== key);
      displayOrder.value = displayOrder.value.filter((k) => k !== key);
      pendingActivityKeys.value = pendingActivityKeys.value.filter(
        (k) => k !== key,
      );
      syncListItemsFromDisplay();
      if (
        nav.conversationId.value === conversationId &&
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
      // 回顶且无待处理活动：仍对齐 canonical，避免长期偏离
      if (
        displayOrder.value.length !== canonicalOrder.value.length ||
        displayOrder.value.some((k, i) => k !== canonicalOrder.value[i])
      ) {
        applyCanonicalOrderNow();
      }
    }
  }

  /** 点击「返回顶部」提示：应用最新排序（滚动由列表组件负责） */
  function jumpToLatestOrder(): void {
    listAtTop.value = true;
    applyCanonicalOrderNow();
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
        // 筛选变化：清阅读上下文，避免 restoreSelection 把旧会话又拉回来
        selected.value = null;
        nav.clearReading();
        // 作废在途请求，但绝不能把 requestVersion 重置为 0（否则新旧请求版本撞车）
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
