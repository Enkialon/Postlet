<script setup lang="ts">
import { computed, inject, onUnmounted, ref, watch } from 'vue';
import { useVirtualizer } from '@tanstack/vue-virtual';
import { navigationKey } from '../../app/provide';
import { useConversations } from './useConversations';
import { useAccounts } from '../accounts/useAccounts';
import {
  displayNameOrAddress,
  formatChatTime,
  formatUnreadBadge,
  truncateText,
} from '../../shared/formatting/format';
import Avatar from '../../shared/ui/Avatar.vue';
import EmptyState from '../../shared/ui/EmptyState.vue';
import ErrorState from '../../shared/ui/ErrorState.vue';
import OverlayScrollArea from '../../shared/ui/OverlayScrollArea.vue';
import type { ConversationKindFilter } from '../../app/navigation';
import type { ConversationListItem } from '../../domain/dto';

const ROW_SIZE = 72;

const nav = inject(navigationKey)!;
const { accountLabel, accounts } = useAccounts();
const { listState, staleWarning, loadMore, loadInitial, openConversation } =
  useConversations();

const showAccountHint = computed(() => accounts.value.length > 1);

const scrollAreaRef = ref<InstanceType<typeof OverlayScrollArea> | null>(null);
const searchQuery = ref('');
const kindOpen = ref(false);
const kindWrapRef = ref<HTMLElement | null>(null);

const kindOptions: { id: ConversationKindFilter; label: string }[] = [
  { id: 'all', label: '全部' },
  { id: 'contact', label: '联系人' },
  { id: 'group', label: '群组' },
];

const kindLabel = computed(
  () =>
    kindOptions.find((o) => o.id === nav.conversationKind.value)?.label ??
    '全部',
);

function titleOf(item: (typeof listState.items)[number]): string {
  if (item.conversation.kind === 'group') {
    return item.group_name?.trim() || '未命名群组';
  }
  return (
    displayNameOrAddress(
      item.contact_display_name,
      item.conversation.normalized_address,
    ) || '未知联系人'
  );
}

function previewOf(item: (typeof listState.items)[number]): string {
  const p = item.last_message_preview;
  if (!p) return '';
  const who = p.is_outbound ? '我: ' : '';
  return `${who}${truncateText(p.body_preview || p.subject, 36)}`;
}

const filteredItems = computed(() => {
  const q = searchQuery.value.trim().toLowerCase();
  if (!q) return listState.items;
  return listState.items.filter((item) => {
    const title = titleOf(item).toLowerCase();
    const addr = (item.conversation.normalized_address || '').toLowerCase();
    const preview = previewOf(item).toLowerCase();
    return title.includes(q) || addr.includes(q) || preview.includes(q);
  });
});

const empty = computed(
  () =>
    listState.loading === 'idle' &&
    !listState.error &&
    listState.items.length === 0,
);

const emptyFiltered = computed(
  () =>
    listState.loading === 'idle' &&
    !listState.error &&
    listState.items.length > 0 &&
    filteredItems.value.length === 0,
);

const scrollViewport = computed(
  () => scrollAreaRef.value?.viewport ?? null,
);

const virtualizer = useVirtualizer(
  computed(() => ({
    count: filteredItems.value.length,
    // 依赖 scrollViewport，确保 Reka Viewport 挂载后 virtualizer 能拿到滚动容器
    getScrollElement: () => scrollViewport.value,
    estimateSize: () => ROW_SIZE,
    overscan: 10,
  })),
);

const virtualItems = computed(() => virtualizer.value.getVirtualItems());
const totalSize = computed(() => virtualizer.value.getTotalSize());

watch(virtualItems, (rows) => {
  const last = rows[rows.length - 1];
  if (!last) return;
  if (
    last.index >= filteredItems.value.length - 8 &&
    listState.hasMore &&
    listState.loading === 'idle'
  ) {
    void loadMore();
  }
});

function toggleKindMenu(): void {
  kindOpen.value = !kindOpen.value;
}

function selectKind(id: ConversationKindFilter): void {
  nav.setConversationKind(id);
  kindOpen.value = false;
}

function onKindDocPointer(e: PointerEvent): void {
  if (!kindOpen.value) return;
  const t = e.target as Node | null;
  if (kindWrapRef.value && t && kindWrapRef.value.contains(t)) return;
  kindOpen.value = false;
}

function onKindKey(e: KeyboardEvent): void {
  if (!kindOpen.value) return;
  if (e.key === 'Escape') {
    e.preventDefault();
    kindOpen.value = false;
  }
}

watch(kindOpen, (open) => {
  if (open) {
    window.addEventListener('pointerdown', onKindDocPointer, true);
    window.addEventListener('keydown', onKindKey);
  } else {
    window.removeEventListener('pointerdown', onKindDocPointer, true);
    window.removeEventListener('keydown', onKindKey);
  }
});

onUnmounted(() => {
  window.removeEventListener('pointerdown', onKindDocPointer, true);
  window.removeEventListener('keydown', onKindKey);
});

function isActive(item: ConversationListItem): boolean {
  return (
    nav.conversationId.value === item.conversation.id &&
    nav.accountId.value === item.conversation.account_id
  );
}

function onSelect(item: ConversationListItem) {
  void openConversation(item.conversation.account_id, item.conversation.id);
}

function onKeySelect(e: KeyboardEvent, item: ConversationListItem) {
  if (e.key === 'Enter') onSelect(item);
}

function itemClass(active: boolean): string {
  return [
    'absolute top-0 left-0 box-border flex w-full cursor-pointer items-center gap-3 px-3 py-3 transition-colors duration-150',
    'hover:bg-surface-sidebar-hover',
    active ? 'bg-surface-sidebar-selected' : '',
  ].join(' ');
}
</script>

<template>
  <section
    class="flex h-full min-h-0 flex-col bg-surface-sidebar"
    aria-label="会话列表"
  >
    <header
      class="flex h-[52px] shrink-0 items-center gap-1.5 border-b border-border-subtle px-3"
    >
      <label
        class="flex h-7 min-w-0 flex-1 items-center rounded-md border border-border-subtle bg-surface-elevated px-2 focus-within:border-brand/35"
      >
        <span class="sr-only">搜索会话</span>
        <input
          v-model="searchQuery"
          type="search"
          class="min-w-0 flex-1 border-0 bg-transparent text-[12px] leading-none text-text-primary outline-none placeholder:text-text-tertiary"
          placeholder="搜索"
          autocomplete="off"
        />
      </label>
      <div ref="kindWrapRef" class="relative shrink-0">
        <button
          type="button"
          class="flex h-7 min-w-[4.25rem] cursor-pointer items-center gap-1 rounded-md border border-border-subtle bg-surface-elevated py-0 pr-1.5 pl-2 text-[12px] text-text-secondary outline-none transition-colors duration-150 hover:bg-surface-sidebar-hover focus-visible:border-brand/35"
          :aria-expanded="kindOpen"
          aria-haspopup="listbox"
          aria-label="会话种类"
          @click="toggleKindMenu"
        >
          <span class="min-w-0 flex-1 text-left">{{ kindLabel }}</span>
          <svg
            class="size-3 shrink-0 text-text-tertiary transition-transform duration-150"
            :class="kindOpen ? 'rotate-180' : ''"
            viewBox="0 0 12 12"
            aria-hidden="true"
          >
            <path
              fill="currentColor"
              d="M2.6 4.2a.75.75 0 0 1 1.06 0L6 6.54l2.34-2.34a.75.75 0 1 1 1.06 1.06l-2.87 2.87a.75.75 0 0 1-1.06 0L2.6 5.26a.75.75 0 0 1 0-1.06Z"
            />
          </svg>
        </button>
        <div
          v-if="kindOpen"
          class="absolute top-[calc(100%+4px)] right-0 z-20 min-w-[7rem] overflow-hidden rounded-lg border border-border-subtle bg-surface-elevated py-1 shadow-panel"
          role="listbox"
          aria-label="会话种类"
        >
          <button
            v-for="opt in kindOptions"
            :key="opt.id"
            type="button"
            role="option"
            class="block w-full cursor-pointer border-0 bg-transparent px-3 py-1.5 text-left text-[12px] transition-colors duration-150 hover:bg-surface-sidebar-hover"
            :class="
              nav.conversationKind.value === opt.id
                ? 'font-medium text-text-primary'
                : 'text-text-secondary'
            "
            :aria-selected="nav.conversationKind.value === opt.id"
            @click="selectKind(opt.id)"
          >
            {{ opt.label }}
          </button>
        </div>
      </div>
    </header>

    <p
      v-if="staleWarning"
      class="m-0 bg-surface-muted px-4 py-2 text-[12px] text-warn"
      role="status"
    >
      内容可能未更新
    </p>

    <ErrorState
      v-if="listState.error && listState.items.length === 0"
      :error="listState.error"
      @retry="loadInitial"
    />

    <EmptyState
      v-else-if="empty"
      title="暂无会话"
      description="同步后会出现在这里"
    />

    <EmptyState
      v-else-if="emptyFiltered"
      title="没有匹配的会话"
      description="试试其他关键词或切换种类"
    />

    <OverlayScrollArea
      v-else
      ref="scrollAreaRef"
      class="m-0 min-h-0 flex-1"
      type="hover"
      role="listbox"
      aria-label="会话"
    >
      <div class="relative w-full" :style="{ height: `${totalSize}px` }">
        <div
          v-for="row in virtualItems"
          :key="
            filteredItems[row.index]
              ? `${filteredItems[row.index]!.conversation.account_id}:${filteredItems[row.index]!.conversation.id}`
              : String(row.key)
          "
          role="option"
          :class="
            itemClass(
              !!filteredItems[row.index] &&
                isActive(filteredItems[row.index]!),
            )
          "
          :aria-selected="
            !!filteredItems[row.index] && isActive(filteredItems[row.index]!)
          "
          tabindex="0"
          :title="
            filteredItems[row.index] ? titleOf(filteredItems[row.index]!) : ''
          "
          :style="{
            height: `${row.size}px`,
            transform: `translateY(${row.start}px)`,
          }"
          @click="
            filteredItems[row.index] && onSelect(filteredItems[row.index]!)
          "
          @keydown="
            filteredItems[row.index] &&
              onKeySelect($event, filteredItems[row.index]!)
          "
        >
          <template v-if="filteredItems[row.index]">
            <Avatar
              :name="titleOf(filteredItems[row.index]!)"
              :kind="
                filteredItems[row.index]!.conversation.kind === 'group'
                  ? 'group'
                  : 'contact'
              "
              :size="40"
            />
            <div class="grid min-w-0 flex-1 gap-0.5">
              <div class="flex items-center gap-2">
                <strong
                  class="min-w-0 flex-1 overflow-hidden text-[14px] font-semibold text-ellipsis whitespace-nowrap text-text-primary"
                  v-no-i18n
                  >{{ titleOf(filteredItems[row.index]!) }}</strong
                >
                <time
                  v-if="filteredItems[row.index]!.conversation.last_message_at"
                  class="shrink-0 text-[11px] text-text-tertiary"
                  :datetime="
                    String(
                      filteredItems[row.index]!.conversation.last_message_at,
                    )
                  "
                >
                  {{
                    formatChatTime(
                      filteredItems[row.index]!.conversation.last_message_at,
                    )
                  }}
                </time>
              </div>
              <div class="flex items-center gap-2">
                <p
                  class="m-0 min-w-0 flex-1 overflow-hidden text-[12px] leading-normal text-ellipsis whitespace-nowrap text-text-secondary"
                  v-no-i18n
                >
                  <span
                    v-if="showAccountHint"
                    class="text-text-tertiary"
                    v-no-i18n
                    >{{
                      accountLabel(
                        filteredItems[row.index]!.conversation.account_id,
                      )
                    }}
                    · </span
                  >{{ previewOf(filteredItems[row.index]!) }}
                </p>
                <span
                  v-if="
                    filteredItems[row.index]!.conversation.unread_count > 0
                  "
                  class="min-w-[1.05rem] shrink-0 rounded-pill bg-unread px-1.5 py-px text-center text-[11px] leading-snug font-medium text-white"
                  :aria-label="`未读 ${filteredItems[row.index]!.conversation.unread_count}`"
                >
                  {{
                    formatUnreadBadge(
                      filteredItems[row.index]!.conversation.unread_count,
                    )
                  }}
                </span>
              </div>
            </div>
          </template>
        </div>
      </div>
    </OverlayScrollArea>

    <div v-if="listState.items.length && listState.error" class="px-4 py-3">
      <ErrorState :error="listState.error" @retry="loadMore" />
    </div>

    <p
      v-if="listState.loading === 'more' || listState.loading === 'initial'"
      class="m-0 px-4 py-3 text-[13px] text-text-secondary"
      aria-live="polite"
    >
      加载中…
    </p>
  </section>
</template>
