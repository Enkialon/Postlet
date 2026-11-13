<script setup lang="ts">
import { computed, inject, ref, watch } from 'vue';
import { navigationKey, portsKey } from '../../app/provide';
import { useConversations } from './useConversations';
import { useMessages } from './useMessages';
import { useGroups } from '../groups/useGroups';
import {
  displayNameOrAddress,
  formatTimestamp,
} from '../../shared/formatting/format';
import Button from '../../shared/ui/Button.vue';
import EmptyState from '../../shared/ui/EmptyState.vue';
import ErrorState from '../../shared/ui/ErrorState.vue';
import Dialog from '../../shared/ui/Dialog.vue';
import MessageCard from './MessageCard.vue';
import type {
  Contact,
  Group,
  GroupParticipant,
  MessageWithRecipients,
} from '../../domain/entities';
import { asAppError } from '../../shared/errors/asAppError';

const nav = inject(navigationKey)!;
const ports = inject(portsKey)!;
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

const detailContact = ref<Contact | null>(null);
const detailGroup = ref<Group | null>(null);
const detailParticipants = ref<GroupParticipant[]>([]);
const detailLastMessage = ref<MessageWithRecipients | null>(null);
const detailError = ref<string | null>(null);
const detailLoading = ref(false);

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
    if (item.conversation.kind === 'contact' && item.conversation.contact_id) {
      detailContact.value = await ports.contact.getContact(
        accountId,
        item.conversation.contact_id,
      );
      detailGroup.value = null;
      detailParticipants.value = [];
    } else if (
      item.conversation.kind === 'group' &&
      item.conversation.group_id
    ) {
      detailGroup.value = await ports.group.getGroup(
        accountId,
        item.conversation.group_id,
      );
      detailParticipants.value = await ports.group.listParticipants(
        accountId,
        item.conversation.group_id,
      );
      detailContact.value = null;
    } else {
      detailContact.value = null;
      detailGroup.value = null;
      detailParticipants.value = [];
    }

    const lastId = item.conversation.last_message_id;
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
    nav.conversationId.value,
    selected.value?.conversation.updated_at,
    selected.value?.conversation.unread_count,
    selected.value?.conversation.last_message_id,
  ],
  () => {
    void loadDetail();
  },
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

function goContactPage() {
  nav.goPage('contacts');
}

function goGroupPage() {
  nav.goPage('groups');
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
        <p v-if="detailLoading" class="meta">加载中…</p>
        <p v-else-if="detailError" class="err">{{ detailError }}</p>
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
          <div v-if="detailContact">
            <dt>联系人</dt>
            <dd v-no-i18n>
              {{
                displayNameOrAddress(
                  detailContact.display_name,
                  detailContact.normalized_address,
                )
              }}
            </dd>
          </div>
          <div v-if="detailGroup">
            <dt>群组</dt>
            <dd v-no-i18n>{{ detailGroup.name || '未命名群组' }}</dd>
          </div>
          <div v-if="detailLastMessage">
            <dt>最近</dt>
            <dd>
              <span v-no-i18n>{{ detailLastMessage.message.subject }}</span>
              <br />
              <span class="meta">
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

        <div v-if="detailParticipants.length" class="members">
          <h4>成员</h4>
          <ul>
            <li
              v-for="p in detailParticipants"
              :key="p.normalized_address"
              v-no-i18n
            >
              {{ displayNameOrAddress(p.display_name, p.normalized_address) }}
            </li>
          </ul>
        </div>

        <div class="detail-actions">
          <Button
            v-if="selected.conversation.kind === 'contact'"
            @click="goContactPage"
          >
            查看联系人
          </Button>
          <Button
            v-if="selected.conversation.kind === 'group'"
            @click="goGroupPage"
          >
            查看群组
          </Button>
          <Button variant="ghost" @click="nav.setDetailOpen(false)">
            关闭
          </Button>
        </div>
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
.reader__detail h3,
.reader__detail h4 {
  margin: 0 0 0.75rem;
  font-size: 1rem;
}
.reader__detail h4 {
  margin-top: 0.75rem;
  font-size: 0.9rem;
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
.members ul {
  list-style: none;
  margin: 0 0 1rem;
  padding: 0;
  display: grid;
  gap: 0.35rem;
  font-size: 0.85rem;
}
.detail-actions {
  display: flex;
  flex-wrap: wrap;
  gap: 0.4rem;
}
.meta {
  color: var(--muted);
  font-size: 0.8rem;
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
