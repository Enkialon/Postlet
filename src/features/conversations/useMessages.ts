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
let mailChangedBound = false;

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
          // 乐观只改展示；真实状态由后端 mutation 确认
          item.message.is_read = isRead;
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

  /** 删除邮件：先乐观标记本地删除意图，再等后端入队；远端确认前不伪造位置事实。 */
  async function deleteMessages(
    messageIds: MessageId[],
    permanent = false,
  ): Promise<void> {
    const accountId = nav.accountId.value;
    if (!accountId || messageIds.length === 0) return;

    markQueue.value = markQueue.value.then(async () => {
      const now = Date.now();
      const rollback: Array<{
        key: string;
        pendingDelete: boolean;
        local_updated_at: number;
      }> = [];
      for (const id of messageIds) {
        const key = cacheKey(accountId, id);
        const item = entityCache.get(key);
        if (!item) continue;
        rollback.push({
          key,
          pendingDelete: item.pending_delete ?? false,
          local_updated_at: item.message.local_updated_at,
        });
        item.pending_delete = true;
        item.message.local_updated_at = now;
        entityCache.set(key, item);
      }
      applyCacheToList(accountId);
      const nextExpanded = new Set(expandedIds.value);
      for (const id of messageIds) nextExpanded.delete(id);
      expandedIds.value = nextExpanded;

      try {
        await ports.message.deleteMessages({
          account_id: accountId,
          message_ids: messageIds,
          permanent,
        });
      } catch (e) {
        for (const snap of rollback) {
          const item = entityCache.get(snap.key);
          if (!item) continue;
          item.pending_delete = snap.pendingDelete;
          item.message.local_updated_at = snap.local_updated_at;
          entityCache.set(snap.key, item);
        }
        applyCacheToList(accountId);
        throw e;
      }
      void conversations.refreshSoft();
    });
    await markQueue.value;
  }

  async function openMessage(messageId: MessageId): Promise<void> {
    expandedIds.value = new Set([messageId]);
    const suppressed = new Set(suppressAutoRead.value);
    suppressed.delete(messageId);
    suppressAutoRead.value = suppressed;

    const accountId = nav.accountId.value;
    if (!accountId) return;
    // 垃圾夹邮件默认不主动拉取正文；待用户显式确认后再加载
    const cachedItem = entityCache.get(cacheKey(accountId, messageId));
    if (cachedItem?.folder_role !== 'junk') {
      await ensureBody(messageId);
    }
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

  /** 发送状态事件：直接按 message_id 重读状态，不依赖其他同步事件。 */
  async function applySendStates(
    accountId: string,
    messageIds: MessageId[],
  ): Promise<void> {
    let changed = false;
    for (const id of messageIds) {
      const key = cacheKey(accountId, id);
      const item = entityCache.get(key);
      if (!item) continue;
      try {
        const state = await ports.compose.getSendState(accountId, id);
        if (item.send_state !== state) {
          item.send_state = state;
          entityCache.set(key, item);
          changed = true;
        }
      } catch {
        // 单封查询失败不阻碍其他状态刷新
      }
    }
    if (changed) applyCacheToList(accountId);
  }

  async function bindMailChanged(): Promise<void> {
    if (mailChangedBound) return;
    mailChangedBound = true;
    await ports.changeEvents.subscribeMailChanged((event) => {
      const accountId = nav.accountId.value;
      const sessionId = nav.sessionId.value;
      if (!accountId || !sessionId || event.account_id !== accountId) return;
      const touchesSession = event.session_ids.includes(sessionId);
      const touchesMessage = event.message_ids.some((id) =>
        entityCache.has(cacheKey(accountId, id)),
      );
      if (!touchesSession && !touchesMessage) return;
      if (event.origin === 'user_action' && event.message_ids.length > 0) {
        void applySendStates(accountId, event.message_ids);
        return;
      }
      void loadInitial();
    });
  }

  if (!watchBound) {
    watchBound = true;
    void bindMailChanged();
    watch(
      () => [
        nav.accountId.value,
        nav.sessionId.value,
        nav.sessionVersion.value,
      ],
      () => {
        // 立刻清空旧线程；loading 保持 initial，避免短暂 EmptyState 把滚动容器卸掉
        // 版本号只在 loadInitial/loadMore 里递增，避免与软刷新双 ++ 把本次加载作废
        messageState.items = [];
        messageState.nextCursor = null;
        messageState.hasMore = false;
        messageState.error = null;
        expandedIds.value = new Set();
        if (!nav.accountId.value || !nav.sessionId.value) {
          Object.assign(messageState, createEmptyPageState());
          return;
        }
        messageState.loading = 'initial';
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
