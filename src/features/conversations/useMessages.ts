import { inject, reactive, ref, watch } from 'vue';
import type { MessageWithRecipients } from '../../domain/entities';
import type { MessageId } from '../../domain/types';
import { portsKey, navigationKey } from '../../app/provide';
import {
  createEmptyPageState,
  type PageState,
} from '../../shared/state/pageState';
import { asAppError } from '../../shared/errors/asAppError';
import { useConversations } from './useConversations';

const PAGE_SIZE = 30;

const messageState = reactive<PageState<MessageWithRecipients>>(
  createEmptyPageState(),
);
const expandedIds = ref<Set<MessageId>>(new Set());
const bodyLoading = ref<Set<MessageId>>(new Set());
const bodyErrors = ref<Map<MessageId, string>>(new Map());
/** 用户显式标未读后，本次展开周期不自动改回已读 */
const suppressAutoRead = ref<Set<MessageId>>(new Set());
const markQueue = ref<Promise<void>>(Promise.resolve());

const entityCache = new Map<string, MessageWithRecipients>();
let watchBound = false;

function cacheKey(accountId: string, messageId: string): string {
  return `${accountId}:${messageId}`;
}

export function useMessages() {
  const ports = inject(portsKey)!;
  const nav = inject(navigationKey)!;
  const conversations = useConversations();

  function upsertCache(accountId: string, item: MessageWithRecipients): void {
    entityCache.set(cacheKey(accountId, item.message.id), item);
  }

  function applyCacheToList(accountId: string): void {
    messageState.items = messageState.items.map((item) => {
      const cached = entityCache.get(cacheKey(accountId, item.message.id));
      return cached ?? item;
    });
  }

  async function loadInitial(): Promise<void> {
    const accountId = nav.accountId.value;
    const sessionId = nav.sessionId.value;
    if (!accountId || !sessionId) {
      Object.assign(messageState, createEmptyPageState());
      expandedIds.value = new Set();
      return;
    }
    const version = ++messageState.requestVersion;
    const session = nav.sessionVersion.value;
    messageState.loading = 'initial';
    messageState.error = null;
    expandedIds.value = new Set();
    suppressAutoRead.value = new Set();
    try {
      const page = await ports.session.listMessages({
        account_id: accountId,
        session_id: sessionId,
        cursor: null,
        limit: PAGE_SIZE,
        direction: 'older',
      });
      if (
        version !== messageState.requestVersion ||
        session !== nav.sessionVersion.value ||
        sessionId !== nav.sessionId.value
      ) {
        return;
      }
      for (const item of page.items) upsertCache(accountId, item);
      messageState.items = [...page.items].reverse();
      messageState.nextCursor = page.next_cursor;
      messageState.hasMore = page.has_more;
      if (page.has_more && !page.next_cursor) {
        messageState.hasMore = false;
        messageState.error = {
          code: 'internal',
          message: '分页契约错误：has_more 但无游标',
          retryable: false,
        };
      }
    } catch (e) {
      if (version !== messageState.requestVersion) return;
      messageState.error = asAppError(e);
    } finally {
      if (version === messageState.requestVersion) messageState.loading = 'idle';
    }
  }

  /**
   * 向上加载更早邮件，插入列表顶部。
   * @returns 实际前置条数（供 UI 保持滚动锚点）
   */
  async function loadMore(): Promise<number> {
    const accountId = nav.accountId.value;
    const sessionId = nav.sessionId.value;
    if (
      !accountId ||
      !sessionId ||
      !messageState.hasMore ||
      messageState.loading !== 'idle'
    ) {
      return 0;
    }
    const version = ++messageState.requestVersion;
    const session = nav.sessionVersion.value;
    messageState.loading = 'more';
    try {
      const page = await ports.session.listMessages({
        account_id: accountId,
        session_id: sessionId,
        cursor: messageState.nextCursor,
        limit: PAGE_SIZE,
        direction: 'older',
      });
      if (
        version !== messageState.requestVersion ||
        session !== nav.sessionVersion.value ||
        sessionId !== nav.sessionId.value
      ) {
        return 0;
      }
      for (const item of page.items) upsertCache(accountId, item);
      const existing = new Set(messageState.items.map((i) => i.message.id));
      const older = [...page.items]
        .reverse()
        .filter((i) => !existing.has(i.message.id));
      messageState.items = [...older, ...messageState.items];
      messageState.nextCursor = page.next_cursor;
      messageState.hasMore = page.has_more;
      return older.length;
    } catch (e) {
      if (version !== messageState.requestVersion) return 0;
      messageState.error = asAppError(e);
      return 0;
    } finally {
      if (version === messageState.requestVersion) messageState.loading = 'idle';
    }
  }

  async function ensureBody(messageId: MessageId): Promise<void> {
    const accountId = nav.accountId.value;
    if (!accountId) return;
    const cached = entityCache.get(cacheKey(accountId, messageId));
    if (cached?.message.body_content !== null) return;
    if (bodyLoading.value.has(messageId)) return;

    bodyLoading.value = new Set(bodyLoading.value).add(messageId);
    bodyErrors.value.delete(messageId);
    try {
      const item = await ports.message.ensureMessageBody(accountId, messageId);
      upsertCache(accountId, item);
      applyCacheToList(accountId);
      bodyErrors.value.delete(messageId);
    } catch (e) {
      const err = asAppError(e);
      if (err.code === 'not_found') {
        messageState.items = messageState.items.filter(
          (i) => i.message.id !== messageId,
        );
        expandedIds.value.delete(messageId);
      }
      bodyErrors.value = new Map(bodyErrors.value).set(
        messageId,
        err.code === 'not_found' ? '邮件已不可用' : err.message,
      );
    } finally {
      const next = new Set(bodyLoading.value);
      next.delete(messageId);
      bodyLoading.value = next;
    }
  }

  async function markRead(
    messageIds: MessageId[],
    isRead: boolean,
  ): Promise<void> {
    const accountId = nav.accountId.value;
    if (!accountId || messageIds.length === 0) return;

    markQueue.value = markQueue.value.then(async () => {
      await ports.message.markRead({
        account_id: accountId,
        message_ids: messageIds,
        is_read: isRead,
      });
      for (const id of messageIds) {
        const key = cacheKey(accountId, id);
        const item = entityCache.get(key);
        if (item) {
          item.message.is_read = isRead;
          item.message.sync_status = 'pending_local';
          entityCache.set(key, item);
        }
        if (!isRead) {
          suppressAutoRead.value = new Set(suppressAutoRead.value).add(id);
        } else {
          const next = new Set(suppressAutoRead.value);
          next.delete(id);
          suppressAutoRead.value = next;
        }
      }
      applyCacheToList(accountId);
      await conversations.refreshSoft();
    });
    await markQueue.value;
  }

  /** 删除邮件：本地标 deleted_remote；是否从气泡列表消失由前端开关决定。 */
  async function deleteMessages(messageIds: MessageId[]): Promise<void> {
    const accountId = nav.accountId.value;
    if (!accountId || messageIds.length === 0) return;

    markQueue.value = markQueue.value.then(async () => {
      await ports.message.deleteMessages({
        account_id: accountId,
        message_ids: messageIds,
      });
      const now = Date.now();
      for (const id of messageIds) {
        const key = cacheKey(accountId, id);
        const item = entityCache.get(key);
        if (item) {
          item.message.sync_status = 'deleted_remote';
          item.message.local_updated_at = now;
          entityCache.set(key, item);
        }
      }
      applyCacheToList(accountId);
      const nextExpanded = new Set(expandedIds.value);
      for (const id of messageIds) nextExpanded.delete(id);
      expandedIds.value = nextExpanded;
      await conversations.refreshSoft();
    });
    await markQueue.value;
  }

  async function openMessage(messageId: MessageId): Promise<void> {
    expandedIds.value = new Set([messageId]);
    const suppressed = new Set(suppressAutoRead.value);
    suppressed.delete(messageId);
    suppressAutoRead.value = suppressed;

    await ensureBody(messageId);
    const accountId = nav.accountId.value;
    if (!accountId) return;
    const item = entityCache.get(cacheKey(accountId, messageId));
    if (
      item &&
      item.message.body_content !== null &&
      !item.message.is_read &&
      !suppressAutoRead.value.has(messageId)
    ) {
      await markRead([messageId], true);
    }
  }

  /** @deprecated 兼容旧名；等同 openMessage */
  async function expandMessage(messageId: MessageId): Promise<void> {
    await openMessage(messageId);
  }

  if (!watchBound) {
    watchBound = true;
    watch(
      () => [
        nav.accountId.value,
        nav.sessionId.value,
        nav.sessionVersion.value,
      ],
      () => {
        messageState.requestVersion += 1;
        messageState.items = [];
        messageState.nextCursor = null;
        messageState.hasMore = false;
        messageState.error = null;
        messageState.loading = 'idle';
        expandedIds.value = new Set();
        void loadInitial();
      },
    );
  }

  return {
    messageState,
    expandedIds,
    bodyLoading,
    bodyErrors,
    loadInitial,
    loadMore,
    openMessage,
    expandMessage,
    ensureBody,
    markRead,
    deleteMessages,
    entityCache,
  };
}
