<script setup lang="ts">
import { computed, inject, ref } from 'vue';
import { navigationKey } from '../../app/provide';
import { useConversations } from './useConversations';
import { useMessages } from './useMessages';
import { useGroups } from '../groups/useGroups';
import {
  displayNameOrAddress,
} from '../../shared/formatting/format';
import Button from '../../shared/ui/Button.vue';
import EmptyState from '../../shared/ui/EmptyState.vue';
import ErrorState from '../../shared/ui/ErrorState.vue';
import Dialog from '../../shared/ui/Dialog.vue';
import MessageCard from './MessageCard.vue';
import type { Group } from '../../domain/entities';

const nav = inject(navigationKey)!;
const { selected } = useConversations();
const {
  messageState,
  expandedIds,
  bodyLoading,
  bodyErrors,
  loadMore,
  expandMessage,
  ensureBody,
  markRead,
  loadInitial,
} = useMessages();
const groups = useGroups();

const moveOpen = ref(false);
const moveThreadId = ref<string | null>(null);
const moveTargets = ref<Group[]>([]);
const moveTargetId = ref('');
const moveBusy = ref(false);

const title = computed(() => {
  const item = selected.value;
  if (!item) return '';
  if (item.conversation.kind === 'group') {
    return item.group_name?.trim() || '未命名群组';
  }
  return (
    displayNameOrAddress(
      item.contact_display_name,
      item.conversation.normalized_address,
    ) || '会话'
  );
});

const showEmptyHint = computed(
  () => !nav.conversationId.value && messageState.loading === 'idle',
);

const showEmptyThread = computed(
  () =>
    Boolean(nav.conversationId.value) &&
    messageState.loading === 'idle' &&
    !messageState.error &&
    messageState.items.length === 0,
);

async function openMove(threadId: string) {
  moveThreadId.value = threadId;
  moveTargets.value = (await groups.loadAllRootGroups()).filter(
    (g) => g.id !== selected.value?.conversation.group_id,
  );
  moveTargetId.value = moveTargets.value[0]?.id ?? '';
  moveOpen.value = true;
}

async function confirmMove() {
  if (!moveThreadId.value || !moveTargetId.value || moveBusy.value) return;
  moveBusy.value = true;
  const ok = await groups.moveThread(moveThreadId.value, moveTargetId.value);
  moveBusy.value = false;
  if (ok) {
    moveOpen.value = false;
    await loadInitial();
  }
}
</script>

<template>
  <section class="reader" aria-label="阅读区">
    <EmptyState
      v-if="showEmptyHint"
      title="选择联系人或群组，查看往来邮件"
    />

    <template v-else-if="nav.conversationId.value">
      <header class="reader__header">
        <Button
          class="reader__back"
          variant="ghost"
          aria-label="返回列表"
          @click="nav.setNarrowShowList(true)"
        >
          返回
        </Button>
        <h2 class="reader__title" v-no-i18n :title="title">{{ title }}</h2>
        <Button
          variant="ghost"
          aria-label="会话详情"
          @click="nav.setDetailOpen(!nav.detailOpen.value)"
        >
          详情
        </Button>
      </header>

      <ErrorState
        v-if="messageState.error && messageState.items.length === 0"
        :error="messageState.error"
        @retry="loadInitial"
      />

      <EmptyState v-else-if="showEmptyThread" title="还没有往来邮件" />

      <div v-else class="reader__messages">
        <MessageCard
          v-for="item in messageState.items"
          :key="item.message.id"
          :item="item"
          :expanded="expandedIds.has(item.message.id)"
          :body-loading="bodyLoading.has(item.message.id)"
          :body-error="bodyErrors.get(item.message.id) ?? null"
          @toggle="expandMessage(item.message.id)"
          @retry-body="ensureBody(item.message.id)"
          @mark-unread="markRead([item.message.id], false)"
          @move-thread="openMove"
        />

        <div v-if="messageState.hasMore" class="reader__more">
          <Button
            :disabled="messageState.loading === 'more'"
            @click="loadMore"
          >
            {{ messageState.loading === 'more' ? '加载中…' : '加载更多' }}
          </Button>
        </div>
      </div>

      <aside
        v-if="nav.detailOpen.value && selected"
        class="reader__detail"
        aria-label="会话详情"
      >
        <h3>会话详情</h3>
        <dl>
          <div>
            <dt>种类</dt>
            <dd>
              {{ selected.conversation.kind === 'group' ? '群组' : '联系人' }}
            </dd>
          </div>
          <div v-if="selected.conversation.normalized_address">
            <dt>地址</dt>
            <dd v-no-i18n>{{ selected.conversation.normalized_address }}</dd>
          </div>
          <div>
            <dt>未读</dt>
            <dd>{{ selected.conversation.unread_count }}</dd>
          </div>
        </dl>
        <Button variant="ghost" @click="nav.setDetailOpen(false)">关闭</Button>
      </aside>
    </template>

    <Dialog
      :open="moveOpen"
      title="迁移线程到群组"
      :confirming="moveBusy"
      @close="moveOpen = false"
      @confirm="confirmMove"
    >
      <p>将迁移该线程的全部邮件到目标根群组。</p>
      <label class="field">
        目标群组
        <select v-model="moveTargetId">
          <option
            v-for="g in moveTargets"
            :key="g.id"
            :value="g.id"
            v-no-i18n
          >
            {{ g.name }}
          </option>
        </select>
      </label>
      <p v-if="groups.actionError.value" class="err">
        {{ groups.actionError.value }}
      </p>
    </Dialog>
  </section>
</template>

<style scoped>
.reader {
  height: 100%;
  position: relative;
  display: flex;
  flex-direction: column;
  min-height: 0;
}
.reader__header {
  display: flex;
  align-items: center;
  gap: 0.5rem;
  padding: 0.65rem 1rem;
  border-bottom: 1px solid var(--border);
  background: var(--surface);
}
.reader__back {
  display: none;
}
.reader__title {
  margin: 0;
  flex: 1;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-size: 1rem;
}
.reader__messages {
  flex: 1;
  overflow: auto;
  padding: 1rem;
  display: grid;
  gap: 0.75rem;
  align-content: start;
}
.reader__more {
  display: flex;
  justify-content: center;
}
.reader__detail {
  position: absolute;
  top: 0;
  right: 0;
  bottom: 0;
  width: min(320px, 100%);
  background: var(--surface);
  border-left: 1px solid var(--border);
  padding: 1rem;
  overflow: auto;
  box-shadow: -8px 0 24px rgba(0, 0, 0, 0.06);
}
.reader__detail h3 {
  margin: 0 0 0.75rem;
  font-size: 1rem;
}
.reader__detail dl {
  display: grid;
  gap: 0.5rem;
  margin: 0 0 1rem;
}
.reader__detail div {
  display: grid;
  grid-template-columns: 3.5rem 1fr;
  gap: 0.5rem;
  font-size: 0.9rem;
}
.reader__detail dt {
  color: var(--muted);
}
.reader__detail dd {
  margin: 0;
}
.field {
  display: grid;
  gap: 0.35rem;
  margin-top: 0.75rem;
}
.field select {
  font: inherit;
  padding: 0.4rem 0.5rem;
}
.err {
  color: var(--danger);
  font-size: 0.85rem;
}

@media (max-width: 760px) {
  .reader__back {
    display: inline-flex;
  }
}
</style>
