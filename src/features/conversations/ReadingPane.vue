<script setup lang="ts">
import { computed, inject, nextTick, ref, watch } from 'vue';
import { useVirtualizer } from '@tanstack/vue-virtual';
import { navigationKey, portsKey } from '../../app/provide';
import { usePreferences } from '../../app/preferences';
import { useConversations } from './useConversations';
import { useMessages } from './useMessages';
import { useAccounts } from '../accounts/useAccounts';
import {
  displayNameOrAddress,
  formatTimestamp,
} from '../../shared/formatting/format';
import Button from '../../shared/ui/Button.vue';
import EmptyState from '../../shared/ui/EmptyState.vue';
import ErrorState from '../../shared/ui/ErrorState.vue';
import Dialog from '../../shared/ui/Dialog.vue';
import OverlayScrollArea from '../../shared/ui/OverlayScrollArea.vue';
import PaneResizeHandle from '../../shared/ui/PaneResizeHandle.vue';
import { useReadingAsideWidth } from '../../shared/ui/useReadingAsideWidth';
import MessageCard from './MessageCard.vue';
import MailReadingPane from './MailReadingPane.vue';
import MailComposer from '../compose/MailComposer.vue';
import type {
  Contact,
  Group,
  GroupParticipant,
  MessageWithRecipients,
} from '../../domain/entities';
import type { ComposeSendResult, SessionListItem } from '../../domain/dto';
import { asAppError } from '../../shared/errors/asAppError';

/** 估算行高（含气泡间距）；实际高度由 measureElement 校正 */
const ROW_SIZE = 148;

const nav = inject(navigationKey)!;
const ports = inject(portsKey)!;
const { preferences, setShowDeletedInThread } = usePreferences();
const { selected, refreshSoft: refreshConversations } = useConversations();
const {
  messageState,
  pendingSends,
  bodyLoading,
  bodyErrors,
  loadMore,
  openMessage,
  ensureBody,
  markRead,
  deleteMessages,
  moveJunk,
  loadInitial,
} = useMessages();
const { accounts, accountLabel } = useAccounts();

const scrollAreaRef = ref<InstanceType<typeof OverlayScrollArea> | null>(null);
const scrollViewport = computed(
  () => scrollAreaRef.value?.viewport ?? null,
);
const composerRef = ref<InstanceType<typeof MailComposer> | null>(null);
const composeFull = ref(false);
const threadPaneRef = ref<HTMLElement | null>(null);
/** Outlook 式右侧邮件属性；有值时侧栏显示邮件详情而非会话详情 */
const mailDetail = ref<MessageWithRecipients | null>(null);
const {
  asideStyle,
  dragging: asideDragging,
  startDrag: startAsideDrag,
} = useReadingAsideWidth(threadPaneRef, () =>
  mailDetail.value ? 'mail' : 'detail',
);
/** 切换会话后只自动贴底一次，避免加载更多时又被拽到底 */
const pendingStickBottom = ref(Boolean(nav.sessionId.value));
let loadingOlder = false;

/** 后端返回含已删；默认前端隐藏本地删除意图与位置失效 */
const displayMessages = computed(() => {
  const effective = messageState.items.filter(
    (item) =>
      item.message.location_status === null ||
      item.message.location_status === 'present',
  );
  if (preferences.showDeletedInThread) return effective;
  return effective.filter(
    (item) =>
      !item.pending_delete && item.message.location_status !== 'missing',
  );
});

const messageVirtualizer = useVirtualizer(
  computed(() => ({
    count: displayMessages.value.length,
    getScrollElement: () => scrollViewport.value,
    estimateSize: () => ROW_SIZE,
    overscan: 8,
    getItemKey: (index: number) =>
      displayMessages.value[index]?.message.id ?? index,
  })),
);

const messageVirtualItems = computed(() =>
  messageVirtualizer.value.getVirtualItems(),
);
const messagesTotalSize = computed(() =>
  messageVirtualizer.value.getTotalSize(),
);

function measureMessageRow(
  el: Element | { $el?: unknown } | null,
): void {
  const node = el instanceof Element ? el : el?.$el;
  if (node instanceof Element) {
    messageVirtualizer.value.measureElement(node);
  }
}

function scrollToLatest(): void {
  const len = displayMessages.value.length;
  if (len === 0) return;
  messageVirtualizer.value.scrollToIndex(len - 1, { align: 'end' });
}

watch(
  () => nav.sessionId.value,
  () => {
    pendingStickBottom.value = true;
  },
);

watch(
  () =>
    [
      messageState.loading,
      displayMessages.value.length,
      nav.sessionId.value,
    ] as const,
  async ([loading, len, conversationId]) => {
    if (!pendingStickBottom.value || !conversationId) return;
    if (loading !== 'idle' || len === 0) return;
    pendingStickBottom.value = false;
    await nextTick();
    scrollToLatest();
    // 布局稳定后再贴一次，避免虚拟列表高度估算偏差
    requestAnimationFrame(() => scrollToLatest());
  },
);

watch(messageVirtualItems, (rows) => {
  const first = rows[0];
  if (!first) return;
  // 聊天：滚近顶部再拉更早的历史
  if (
    first.index <= 2 &&
    messageState.hasMore &&
    messageState.loading === 'idle' &&
    !loadingOlder
  ) {
    void loadOlderPreservingScroll();
  }
});

async function loadOlderPreservingScroll(): Promise<void> {
  if (loadingOlder) return;
  loadingOlder = true;
  const el = scrollViewport.value;
  const beforeSize = messageVirtualizer.value.getTotalSize();
  const beforeTop = el?.scrollTop ?? 0;
  try {
    const added = await loadMore();
    if (added <= 0 || !el) return;
    await nextTick();
    const afterSize = messageVirtualizer.value.getTotalSize();
    el.scrollTop = beforeTop + (afterSize - beforeSize);
  } finally {
    loadingOlder = false;
  }
}

const reassignOpen = ref(false);
const reassignMessageId = ref<string | null>(null);
const reassignTargets = ref<SessionListItem[]>([]);
const reassignTargetId = ref('');
const reassignBusy = ref(false);
const reassignError = ref<string | null>(null);

const deleteOpen = ref(false);
const deleteMessageId = ref<string | null>(null);
const deleteBusy = ref(false);
const deleteError = ref<string | null>(null);

/** 标为垃圾 / 移出垃圾：失败时保留原展示并提示 */
const junkMoveBusy = ref(false);
const junkMoveError = ref<string | null>(null);

async function onMoveJunk(direction: 'to_junk' | 'out_of_junk'): Promise<void> {
  const item = mailDetail.value;
  if (!item || junkMoveBusy.value) return;
  junkMoveBusy.value = true;
  junkMoveError.value = null;
  try {
    await moveJunk(item.message.id, direction);
    const next = messageState.items.find(
      (i) => i.message.id === item.message.id,
    );
    if (next) mailDetail.value = next;
  } catch (e) {
    junkMoveError.value = asAppError(e).message;
  } finally {
    junkMoveBusy.value = false;
  }
}

function sessionOptionLabel(item: SessionListItem): string {
  const kind = item.session.kind === 'group' ? '群聊' : '私聊';
  const name =
    item.display_name?.trim() ||
    item.group_name?.trim() ||
    item.contact_display_name?.trim() ||
    item.session.normalized_address?.trim() ||
    item.session.id;
  return `${kind} · ${name}`;
}

async function loadReassignTargets(): Promise<SessionListItem[]> {
  const accountId = nav.accountId.value;
  if (!accountId) return [];
  const all: SessionListItem[] = [];
  let cursor: string | null = null;
  let hasMore = true;
  while (hasMore) {
    const page = await ports.session.listSessions({
      account_id: accountId,
      cursor,
      limit: 50,
      kind: null,
    });
    all.push(...page.items);
    hasMore = page.has_more;
    cursor = page.next_cursor;
    if (hasMore && !cursor) break;
  }
  const currentId = nav.sessionId.value;
  return all.filter((s) => s.session.id !== currentId);
}

const detailContact = ref<Contact | null>(null);
const detailGroup = ref<Group | null>(null);
const detailParticipants = ref<GroupParticipant[]>([]);
const detailLastMessage = ref<MessageWithRecipients | null>(null);
const detailError = ref<string | null>(null);
const detailLoading = ref(false);

const showingMailDetail = computed(
  () => nav.detailOpen.value && mailDetail.value != null,
);

const title = computed(() => {
  const item = selected.value;
  // 选中态必须与导航一致，避免点会话后标题/线程短暂错位
  if (
    !item ||
    item.session.id !== nav.sessionId.value ||
    item.session.account_id !== nav.accountId.value
  ) {
    return '';
  }
  const resolved = item.display_name?.trim();
  if (resolved) return resolved;
  if (item.session.kind === 'group') {
    return item.group_name?.trim() || '未命名群组';
  }
  const fromMail =
    item.last_message_preview && !item.last_message_preview.is_outbound
      ? item.last_message_preview.from_name
      : null;
  return (
    displayNameOrAddress(
      item.contact_display_name,
      item.session.normalized_address,
      fromMail,
    ) || '会话'
  );
});

const ownerAccountLabel = computed(() => {
  if (accounts.value.length < 2) return null;
  const id = selected.value?.session.account_id;
  return id ? accountLabel(id) : null;
});

const showEmptyHint = computed(
  () => !nav.sessionId.value && messageState.loading === 'idle',
);

const showEmptyThread = computed(
  () =>
    Boolean(nav.sessionId.value) &&
    messageState.loading === 'idle' &&
    !messageState.error &&
    displayMessages.value.length === 0 &&
    pendingSends.value.length === 0,
);

async function loadDetail(): Promise<void> {
  const accountId = nav.accountId.value;
  const item = selected.value;
  if (!accountId || !item || !nav.detailOpen.value) {
    detailContact.value = null;
    detailGroup.value = null;
    detailParticipants.value = [];
    detailLastMessage.value = null;
    detailError.value = null;
    return;
  }

  detailLoading.value = true;
  detailError.value = null;
  try {
    if (item.session.kind === 'private' && item.session.contact_id) {
      detailContact.value = await ports.contact.getContact(
        accountId,
        item.session.contact_id,
      );
      detailGroup.value = null;
      detailParticipants.value = [];
    } else if (
      item.session.kind === 'group' &&
      item.session.group_id
    ) {
      detailGroup.value = await ports.group.getGroup(
        accountId,
        item.session.group_id,
      );
      detailParticipants.value = await ports.group.listParticipants(
        accountId,
        item.session.group_id,
      );
      detailContact.value = null;
    } else {
      detailContact.value = null;
      detailGroup.value = null;
      detailParticipants.value = [];
    }

    const lastId = item.session.last_message_id;
    if (lastId) {
      detailLastMessage.value = await ports.message.getMessage(
        accountId,
        lastId,
      );
    } else {
      detailLastMessage.value = null;
    }
  } catch (e) {
    detailError.value = asAppError(e).message;
  } finally {
    detailLoading.value = false;
  }
}

watch(
  () => [
    nav.detailOpen.value,
    nav.sessionId.value,
    selected.value?.session.updated_at,
    selected.value?.session.unread_count,
    selected.value?.session.last_message_id,
  ],
  () => {
    void loadDetail();
  },
);

async function openReassign(messageId: string) {
  reassignMessageId.value = messageId;
  reassignError.value = null;
  try {
    reassignTargets.value = await loadReassignTargets();
    reassignTargetId.value = reassignTargets.value[0]?.session.id ?? '';
    reassignOpen.value = true;
  } catch (e) {
    reassignError.value = asAppError(e).message;
    reassignOpen.value = true;
  }
}

async function confirmReassign() {
  const accountId = nav.accountId.value;
  if (
    !accountId ||
    !reassignMessageId.value ||
    !reassignTargetId.value ||
    reassignBusy.value
  ) {
    return;
  }
  reassignBusy.value = true;
  reassignError.value = null;
  try {
    await ports.session.reassignMessages({
      account_id: accountId,
      message_ids: [reassignMessageId.value],
      target_session_id: reassignTargetId.value,
    });
    reassignOpen.value = false;
    await loadInitial();
    await refreshConversations();
  } catch (e) {
    reassignError.value = asAppError(e).message;
  } finally {
    reassignBusy.value = false;
  }
}

function goContactPage() {
  nav.goPage('contacts');
}

function goGroupPage() {
  nav.goPage('groups');
}

function toggleConversationDetail(): void {
  if (nav.detailOpen.value && !mailDetail.value) {
    nav.setDetailOpen(false);
    return;
  }
  mailDetail.value = null;
  nav.setDetailOpen(true);
}

/** 左键打开：右侧渲染正文 + 邮件头；保留 ensureBody / 自动已读 */
async function openMailReading(item: MessageWithRecipients): Promise<void> {
  mailDetail.value = item;
  nav.selectMessage(item.message.id);
  nav.setDetailOpen(true);
  const accountId = nav.accountId.value;
  const sessionVersion = nav.sessionVersion.value;
  await openMessage(item.message.id);
  if (
    mailDetail.value?.message.id !== item.message.id ||
    nav.accountId.value !== accountId ||
    nav.sessionVersion.value !== sessionVersion
  ) return;
  // openMessage 可能更新了缓存，侧栏跟进最新快照
  const next = messageState.items.find((i) => i.message.id === item.message.id);
  if (next) mailDetail.value = next;
}

function closeDetailPanel(): void {
  mailDetail.value = null;
  nav.selectMessage(null);
  nav.setDetailOpen(false);
}

function onComposeModeChange(next: 'compact' | 'full'): void {
  composeFull.value = next === 'full';
  if (next === 'full') {
    mailDetail.value = null;
    nav.selectMessage(null);
    nav.setDetailOpen(false);
  }
}

function onReplyMessage(item: MessageWithRecipients): void {
  composerRef.value?.beginReply(item);
}

function requestDeleteMessage(messageId: string): void {
  deleteMessageId.value = messageId;
  deleteError.value = null;
  deleteOpen.value = true;
}

async function confirmDeleteMessage(): Promise<void> {
  const messageId = deleteMessageId.value;
  if (!messageId || deleteBusy.value) return;
  deleteBusy.value = true;
  deleteError.value = null;
  try {
    if (mailDetail.value?.message.id === messageId) {
      closeDetailPanel();
    }
    await deleteMessages([messageId]);
    deleteOpen.value = false;
    deleteMessageId.value = null;
  } catch (e) {
    deleteError.value = asAppError(e).message;
  } finally {
    deleteBusy.value = false;
  }
}

async function onMailSent(_result: ComposeSendResult): Promise<void> {
  pendingStickBottom.value = true;
  await loadInitial();
  void refreshConversations();
}

const mailBodyLoading = computed(() => {
  const id = mailDetail.value?.message.id;
  return id ? bodyLoading.value.has(id) : false;
});

const mailBodyError = computed(() => {
  const id = mailDetail.value?.message.id;
  return id ? (bodyErrors.value.get(id) ?? null) : null;
});

watch(
  () => nav.sessionId.value,
  () => {
    mailDetail.value = null;
  },
);

// 列表刷新后保持侧栏指向同一封信的最新快照
watch(
  () => messageState.items,
  (items) => {
    const current = mailDetail.value;
    if (!current) return;
    const next = items.find((i) => i.message.id === current.message.id);
    if (next) mailDetail.value = next;
  },
);
</script>
<template>
  <section class="flex h-full min-h-0 flex-col bg-surface-chat" aria-label="阅读区">
    <EmptyState
      v-if="showEmptyHint"
      title="选择一位联系人开始看信"
      description="左侧会话列表像微信聊天一样组织往来"
    />

    <template v-else-if="nav.sessionId.value">
      <header
        class="flex h-[52px] shrink-0 items-center gap-2 border-b border-border-subtle bg-surface-header px-5"
      >
        <Button
          class="hidden max-[760px]:inline-flex"
          variant="ghost"
          aria-label="返回列表"
          @click="nav.setNarrowShowList(true)"
        >
          返回
        </Button>
        <div class="min-w-0 flex-1">
          <h2
            class="m-0 overflow-hidden text-left text-base font-semibold text-ellipsis whitespace-nowrap text-text-primary"
            v-no-i18n
            :title="title"
          >
            {{ title }}
          </h2>
          <p
            v-if="ownerAccountLabel"
            class="m-0 overflow-hidden text-[11px] text-ellipsis whitespace-nowrap text-text-tertiary"
            v-no-i18n
          >
            {{ ownerAccountLabel }}
          </p>
        </div>
        <Button
          variant="ghost"
          aria-label="会话详情"
          @click="toggleConversationDetail"
        >
          ···
        </Button>
      </header>

      <div
        ref="threadPaneRef"
        class="relative flex min-h-0 flex-1 flex-col"
      >
        <ErrorState
          v-if="messageState.error && messageState.items.length === 0"
          :error="messageState.error"
          @retry="loadInitial"
        />

        <EmptyState
          v-else-if="showEmptyThread"
          class="min-h-0 flex-1"
          title="还没有往来邮件"
        />

        <OverlayScrollArea
          v-else
          :key="`${nav.accountId.value ?? ''}:${nav.sessionId.value ?? ''}`"
          ref="scrollAreaRef"
          class="min-h-0 flex-1 bg-surface-chat"
          type="hover"
        >
          <!-- 以聊天栏自身宽度为容器，气泡用 @container/chat 联动 -->
          <div
            class="@container/chat px-2.5 py-3 @[28rem]/chat:px-4 @[28rem]/chat:py-4 @[40rem]/chat:px-5 @[40rem]/chat:py-5"
          >
            <section
              v-if="pendingSends.length > 0"
              class="mb-5 rounded-lg border border-border-subtle bg-surface-elevated p-3"
              aria-label="发信记录"
            >
              <p
                class="m-0 mb-2 text-[12px] font-medium tracking-wide text-text-tertiary uppercase"
              >
                发信记录
              </p>
              <div class="flex flex-col gap-3">
                <MessageCard
                  v-for="item in pendingSends"
                  :key="item.message.id"
                  :item="item"
                  :selected="mailDetail?.message.id === item.message.id"
                  :preferred-name="selected?.contact_display_name"
                  @select="openMailReading(item)"
                  @delete="requestDeleteMessage(item.message.id)"
                  @reassign="openReassign"
                  @reply="onReplyMessage(item)"
                />
              </div>
            </section>
            <div
              class="relative w-full min-w-0"
              :style="{ height: `${messagesTotalSize}px` }"
            >
              <div
                v-for="row in messageVirtualItems"
                :key="String(row.key)"
                class="absolute top-0 left-0 box-border w-full min-w-0 px-0.5 pb-6 @[28rem]/chat:px-1 @[28rem]/chat:pb-8"
                :data-index="row.index"
                :ref="measureMessageRow"
                :style="{
                  transform: `translateY(${row.start}px)`,
                }"
              >
                <MessageCard
                  v-if="displayMessages[row.index]"
                  :item="displayMessages[row.index]!"
                  :selected="
                    mailDetail?.message.id ===
                    displayMessages[row.index]!.message.id
                  "
                  :preferred-name="selected?.contact_display_name"
                  @select="openMailReading(displayMessages[row.index]!)"
                  @reply="onReplyMessage(displayMessages[row.index]!)"
                  @mark-unread="
                    markRead([displayMessages[row.index]!.message.id], false)
                  "
                  @delete="
                    requestDeleteMessage(
                      displayMessages[row.index]!.message.id,
                    )
                  "
                  @reassign="openReassign"
                />
              </div>
            </div>
            <p
              v-if="messageState.loading === 'more'"
              class="sticky top-0 z-[2] mx-auto my-0 w-fit rounded-pill bg-surface-chat px-3 py-1.5 text-center text-[12px] text-text-secondary"
              aria-live="polite"
            >
              加载更早邮件…
            </p>
          </div>
        </OverlayScrollArea>

        <div
          v-if="!composeFull && nav.detailOpen.value && (mailDetail || selected)"
          class="absolute inset-y-0 right-0 z-[4] flex"
          :style="asideStyle"
        >
          <PaneResizeHandle
            orientation="vertical"
            :dragging="asideDragging"
            :label="showingMailDetail ? '调整邮件阅读宽度' : '调整会话详情宽度'"
            @start="startAsideDrag"
          />
          <aside
            class="flex min-h-0 min-w-0 flex-1 flex-col border-l border-border-subtle bg-surface-elevated"
            :class="
              showingMailDetail
                ? 'overflow-hidden p-0'
                : 'overflow-auto p-4'
            "
            :aria-label="showingMailDetail ? '邮件阅读' : '会话详情'"
          >
          <MailReadingPane
            v-if="mailDetail"
            :item="mailDetail"
            :body-loading="mailBodyLoading"
            :body-error="mailBodyError"
            :junk-move-busy="junkMoveBusy"
            :junk-move-error="junkMoveError"
            :preferred-name="selected?.contact_display_name"
            @close="closeDetailPanel"
            @retry-body="ensureBody(mailDetail.message.id)"
            @delete="requestDeleteMessage(mailDetail.message.id)"
            @move-junk="onMoveJunk"
          />

          <template v-else-if="selected">
            <div class="mb-3 flex shrink-0 items-start justify-between gap-2">
              <h3 class="m-0 text-base font-semibold text-text-primary">
                会话详情
              </h3>
              <Button
                variant="ghost"
                aria-label="关闭"
                @click="closeDetailPanel"
              >
                ✕
              </Button>
            </div>
            <p v-if="detailLoading" class="text-[12px] text-text-secondary">
              加载中…
            </p>
            <p v-else-if="detailError" class="text-[13px] text-danger">
              {{ detailError }}
            </p>
            <label
              class="mb-4 flex cursor-pointer items-center gap-2 text-[13px] text-text-primary"
            >
              <input
                type="checkbox"
                class="size-3.5 accent-brand"
                :checked="preferences.showDeletedInThread"
                @change="
                  setShowDeletedInThread(
                    ($event.target as HTMLInputElement).checked,
                  )
                "
              />
              显示已删除邮件
            </label>
            <dl class="mb-4 grid gap-2">
              <div
                class="grid grid-cols-[3.5rem_1fr] items-start gap-2 text-[14px]"
              >
                <dt class="text-text-secondary">种类</dt>
                <dd class="m-0 break-words text-text-primary">
                  {{
                    selected.session.kind === 'group' ? '群组' : '私聊'
                  }}
                </dd>
              </div>
              <div
                v-if="selected.session.normalized_address"
                class="grid grid-cols-[3.5rem_1fr] items-start gap-2 text-[14px]"
              >
                <dt class="text-text-secondary">地址</dt>
                <dd class="m-0 break-words text-text-primary" v-no-i18n>
                  {{ selected.session.normalized_address }}
                </dd>
              </div>
              <div
                class="grid grid-cols-[3.5rem_1fr] items-start gap-2 text-[14px]"
              >
                <dt class="text-text-secondary">未读</dt>
                <dd class="m-0 break-words text-text-primary">
                  {{ selected.session.unread_count }}
                </dd>
              </div>
              <div
                v-if="detailContact"
                class="grid grid-cols-[3.5rem_1fr] items-start gap-2 text-[14px]"
              >
                <dt class="text-text-secondary">联系人</dt>
                <dd class="m-0 break-words text-text-primary" v-no-i18n>
                  {{
                    displayNameOrAddress(
                      detailContact.display_name,
                      detailContact.normalized_address,
                    )
                  }}
                </dd>
              </div>
              <div
                v-if="detailGroup"
                class="grid grid-cols-[3.5rem_1fr] items-start gap-2 text-[14px]"
              >
                <dt class="text-text-secondary">群组</dt>
                <dd class="m-0 break-words text-text-primary" v-no-i18n>
                  {{ detailGroup.name || '未命名群组' }}
                </dd>
              </div>
              <div
                v-if="detailLastMessage"
                class="grid grid-cols-[3.5rem_1fr] items-start gap-2 text-[14px]"
              >
                <dt class="text-text-secondary">最近</dt>
                <dd class="m-0 break-words text-text-primary">
                  <span v-no-i18n>{{ detailLastMessage.message.subject }}</span>
                  <br />
                  <span class="text-[12px] text-text-tertiary">
                    {{
                      formatTimestamp(
                        detailLastMessage.message.received_at ??
                          detailLastMessage.message.sent_at,
                      )
                    }}
                  </span>
                </dd>
              </div>
            </dl>

            <div v-if="detailParticipants.length">
              <h4 class="my-3 text-[14px] font-medium text-text-primary">
                成员
              </h4>
              <ul class="mb-4 grid list-none gap-1.5 p-0 text-[13px]">
                <li
                  v-for="p in detailParticipants"
                  :key="p.normalized_address"
                  v-no-i18n
                >
                  {{
                    displayNameOrAddress(p.display_name, p.normalized_address)
                  }}
                </li>
              </ul>
            </div>

            <div class="flex flex-wrap gap-2">
              <Button
                v-if="selected.session.kind === 'private'"
                @click="goContactPage"
              >
                查看联系人
              </Button>
              <Button
                v-if="selected.session.kind === 'group'"
                @click="goGroupPage"
              >
                查看群组
              </Button>
              <Button variant="ghost" @click="closeDetailPanel">关闭</Button>
            </div>
          </template>
          </aside>
        </div>

        <MailComposer
          ref="composerRef"
          :require-recipients-for-send="
            selected?.session.kind === 'group'
          "
          @sent="onMailSent"
          @mode-change="onComposeModeChange"
        />
      </div>
    </template>

    <Dialog
      :open="deleteOpen"
      title="删除邮件"
      confirm-label="删除"
      :confirming="deleteBusy"
      danger
      @close="
        deleteOpen = false;
        deleteMessageId = null;
        deleteError = null;
      "
      @confirm="confirmDeleteMessage"
    >
      <p>确定删除这封邮件？将同步到远端，可在「显示已删除」中查看。</p>
      <p v-if="deleteError" class="mt-2 text-[13px] text-danger">
        {{ deleteError }}
      </p>
    </Dialog>

    <Dialog
      :open="reassignOpen"
      title="改到其他会话"
      :confirming="reassignBusy"
      @close="reassignOpen = false"
      @confirm="confirmReassign"
    >
      <p>将该邮件改挂到目标会话（私聊或群聊均可）。</p>
      <label class="mt-3 grid gap-1.5 text-[13px]">
        目标会话
        <select
          v-model="reassignTargetId"
          class="rounded-md border border-border-subtle bg-surface-input px-2.5 py-2 text-[14px]"
        >
          <option
            v-for="s in reassignTargets"
            :key="s.session.id"
            :value="s.session.id"
            v-no-i18n
          >
            {{ sessionOptionLabel(s) }}
          </option>
        </select>
      </label>
      <p
        v-if="reassignTargets.length === 0 && !reassignError"
        class="mt-2 text-[13px] text-text-secondary"
      >
        当前没有可迁移的其他会话。
      </p>
      <p v-if="reassignError" class="mt-2 text-[13px] text-danger">
        {{ reassignError }}
      </p>
    </Dialog>
  </section>
</template>
