<script setup lang="ts">
import { computed, inject } from 'vue';
import { navigationKey } from '../../app/provide';
import { useConversations } from './useConversations';
import {
  displayNameOrAddress,
  formatTimestamp,
  formatUnreadBadge,
  truncateText,
} from '../../shared/formatting/format';
import Button from '../../shared/ui/Button.vue';
import EmptyState from '../../shared/ui/EmptyState.vue';
import ErrorState from '../../shared/ui/ErrorState.vue';
import type { ConversationKindFilter } from '../../app/navigation';

const nav = inject(navigationKey)!;
const { listState, staleWarning, loadMore, loadInitial, openConversation } =
  useConversations();

const filters: { id: ConversationKindFilter; label: string }[] = [
  { id: 'all', label: '全部' },
  { id: 'contact', label: '联系人' },
  { id: 'group', label: '群组' },
];

const empty = computed(
  () =>
    listState.loading === 'idle' &&
    !listState.error &&
    listState.items.length === 0,
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

function onFilter(kind: ConversationKindFilter) {
  nav.setConversationKind(kind);
}

function onSelect(id: string) {
  void openConversation(id);
}

function onKeySelect(e: KeyboardEvent, id: string) {
  if (e.key === 'Enter') onSelect(id);
}
</script>

<template>
  <section class="list-pane" aria-label="会话列表">
    <header class="list-pane__filters" role="tablist" aria-label="种类筛选">
      <button
        v-for="f in filters"
        :key="f.id"
        type="button"
        role="tab"
        class="filter"
        :class="{ 'filter--active': nav.conversationKind.value === f.id }"
        :aria-selected="nav.conversationKind.value === f.id"
        @click="onFilter(f.id)"
      >
        {{ f.label }}
      </button>
    </header>

    <p v-if="staleWarning" class="stale" role="status">内容可能未更新</p>

    <ErrorState
      v-if="listState.error && listState.items.length === 0"
      :error="listState.error"
      @retry="loadInitial"
    />

    <EmptyState v-else-if="empty" title="尚无会话" />

    <ul v-else class="items" role="listbox" aria-label="会话">
      <li
        v-for="item in listState.items"
        :key="item.conversation.id"
        role="option"
        class="item"
        :class="{
          'item--active': nav.conversationId.value === item.conversation.id,
          'item--unread': item.conversation.unread_count > 0,
        }"
        :aria-selected="nav.conversationId.value === item.conversation.id"
        tabindex="0"
        :title="titleOf(item)"
        @click="onSelect(item.conversation.id)"
        @keydown="onKeySelect($event, item.conversation.id)"
      >
        <div class="item__row">
          <span
            class="item__kind"
            :aria-label="item.conversation.kind === 'group' ? '群组' : '联系人'"
          >
            {{ item.conversation.kind === 'group' ? '群' : '人' }}
          </span>
          <strong class="item__title">{{ titleOf(item) }}</strong>
          <span
            v-if="item.conversation.unread_count > 0"
            class="item__badge"
            :aria-label="`未读 ${item.conversation.unread_count}`"
          >
            {{ formatUnreadBadge(item.conversation.unread_count) }}
          </span>
        </div>
        <p class="item__preview">
          <template v-if="item.last_message_preview">
            {{ truncateText(item.last_message_preview.subject) }}
            —
            {{ truncateText(item.last_message_preview.body_preview, 48) }}
          </template>
        </p>
        <time
          v-if="item.conversation.last_message_at"
          class="item__time"
          :datetime="String(item.conversation.last_message_at)"
        >
          {{ formatTimestamp(item.conversation.last_message_at) }}
        </time>
      </li>
    </ul>

    <div v-if="listState.items.length && listState.error" class="footer-error">
      <ErrorState :error="listState.error" @retry="loadMore" />
    </div>

    <div v-if="listState.hasMore" class="more">
      <Button
        :disabled="listState.loading === 'more'"
        @click="loadMore"
      >
        {{ listState.loading === 'more' ? '加载中…' : '加载更多' }}
      </Button>
    </div>

    <p
      v-if="listState.loading === 'initial'"
      class="loading"
      aria-live="polite"
    >
      加载中…
    </p>
  </section>
</template>

<style scoped>
.list-pane {
  height: 100%;
  display: flex;
  flex-direction: column;
  min-height: 0;
}
.list-pane__filters {
  display: flex;
  gap: 0.25rem;
  padding: 0.65rem 0.75rem;
  border-bottom: 1px solid var(--border);
}
.filter {
  border: 0;
  background: transparent;
  padding: 0.35rem 0.65rem;
  border-radius: 6px;
  cursor: pointer;
  color: var(--muted);
  font: inherit;
}
.filter--active {
  background: var(--surface-2);
  color: var(--text);
  font-weight: 600;
}
.stale {
  margin: 0;
  padding: 0.4rem 0.75rem;
  font-size: 0.8rem;
  color: var(--warn);
  background: color-mix(in srgb, var(--warn) 12%, transparent);
}
.items {
  list-style: none;
  margin: 0;
  padding: 0.35rem;
  overflow: auto;
  flex: 1;
}
.item {
  padding: 0.7rem 0.65rem;
  border-radius: 8px;
  cursor: pointer;
  border: 1px solid transparent;
}
.item:hover {
  background: var(--surface-2);
}
.item--active {
  background: color-mix(in srgb, var(--accent) 12%, var(--surface));
  border-color: color-mix(in srgb, var(--accent) 35%, transparent);
}
.item__row {
  display: flex;
  align-items: center;
  gap: 0.45rem;
}
.item__kind {
  width: 1.35rem;
  height: 1.35rem;
  border-radius: 4px;
  display: grid;
  place-items: center;
  font-size: 0.7rem;
  background: var(--surface-2);
  color: var(--muted);
  flex: none;
}
.item__title {
  flex: 1;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-size: 0.92rem;
}
.item--unread .item__title {
  font-weight: 700;
}
.item__badge {
  font-size: 0.72rem;
  background: var(--accent);
  color: #fff;
  border-radius: 999px;
  padding: 0.05rem 0.4rem;
  min-width: 1.2rem;
  text-align: center;
}
.item__preview {
  margin: 0.25rem 0 0 1.8rem;
  color: var(--muted);
  font-size: 0.8rem;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.item__time {
  display: block;
  margin: 0.2rem 0 0 1.8rem;
  font-size: 0.72rem;
  color: var(--muted);
}
.more,
.loading,
.footer-error {
  padding: 0.65rem 0.75rem;
}
.loading {
  margin: 0;
  color: var(--muted);
}
</style>
