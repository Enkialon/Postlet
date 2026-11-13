<script setup lang="ts">
import type { MessageWithRecipients } from '../../domain/entities';
import {
  displayNameOrAddress,
  formatTimestampOrUnknown,
} from '../../shared/formatting/format';
import Button from '../../shared/ui/Button.vue';
import MailBodyView from '../../shared/mail-content/MailBodyView.vue';

const props = defineProps<{
  item: MessageWithRecipients;
  expanded: boolean;
  bodyLoading: boolean;
  bodyError: string | null;
}>();

const emit = defineEmits<{
  toggle: [];
  retryBody: [];
  markUnread: [];
  moveThread: [threadId: string];
}>();

function recipientsLine(): string {
  return props.item.recipients
    .filter((r) => r.role === 'to' || r.role === 'cc')
    .map((r) => displayNameOrAddress(r.display_name, r.normalized_address))
    .join(', ');
}
</script>

<template>
  <article
    class="card"
    :class="{ 'card--unread': !item.message.is_read, 'card--open': expanded }"
  >
    <header class="card__head" @click="emit('toggle')">
      <div class="card__meta">
        <strong class="card__from" v-no-i18n>
          {{
            displayNameOrAddress(
              item.message.from_name,
              item.message.from_address,
            )
          }}
        </strong>
        <span
          v-if="!item.message.is_read"
          class="card__unread-mark"
          aria-label="未读"
          >●</span
        >
        <time class="card__time">
          {{
            formatTimestampOrUnknown(
              item.message.received_at ?? item.message.sent_at,
            )
          }}
        </time>
      </div>
      <h3 class="card__subject" v-no-i18n>{{ item.message.subject }}</h3>
      <p v-if="!expanded" class="card__preview" v-no-i18n>
        {{ item.message.body_preview }}
      </p>
      <p v-if="item.message.has_attachments" class="card__attach">含附件</p>
      <p
        v-if="item.message.sync_status === 'pending_local'"
        class="card__pending"
      >
        等待同步
      </p>
    </header>

    <div v-if="expanded" class="card__body">
      <dl class="card__details">
        <div>
          <dt>收件人</dt>
          <dd v-no-i18n>{{ recipientsLine() || '—' }}</dd>
        </div>
        <div v-if="item.message.parent_folder_name">
          <dt>文件夹</dt>
          <dd v-no-i18n>{{ item.message.parent_folder_name }}</dd>
        </div>
      </dl>

      <div v-if="bodyLoading" class="card__loading" aria-live="polite">
        正在加载正文…
      </div>
      <div v-else-if="bodyError" class="card__error">
        <p>{{ bodyError }}</p>
        <Button v-if="bodyError !== '邮件已不可用'" @click="emit('retryBody')">
          重试
        </Button>
      </div>
      <p
        v-else-if="item.message.body_content === ''"
        class="card__empty-body"
      >
        此邮件正文为空
      </p>
      <MailBodyView
        v-else-if="item.message.body_content != null"
        :content="item.message.body_content"
        :content-type="item.message.body_content_type"
      />

      <div class="card__actions">
        <Button variant="ghost" @click="emit('markUnread')">标记未读</Button>
        <Button
          v-if="item.message.thread_id"
          variant="ghost"
          @click="emit('moveThread', item.message.thread_id)"
        >
          迁移线程
        </Button>
      </div>
    </div>
  </article>
</template>

<style scoped>
.card {
  border: 1px solid var(--border);
  border-radius: 10px;
  background: var(--surface);
  overflow: hidden;
}
.card--unread {
  border-left: 3px solid var(--accent);
}
.card__head {
  padding: 0.85rem 1rem;
  cursor: pointer;
}
.card__meta {
  display: flex;
  align-items: center;
  gap: 0.5rem;
  margin-bottom: 0.25rem;
}
.card__from {
  flex: 1;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.card__unread-mark {
  color: var(--accent);
  font-size: 0.65rem;
}
.card__time {
  color: var(--muted);
  font-size: 0.78rem;
}
.card__subject {
  margin: 0;
  font-size: 0.98rem;
  font-weight: 600;
}
.card__preview {
  margin: 0.35rem 0 0;
  color: var(--muted);
  font-size: 0.85rem;
}
.card__attach,
.card__pending {
  margin: 0.35rem 0 0;
  font-size: 0.75rem;
  color: var(--muted);
}
.card__body {
  padding: 0 1rem 1rem;
  border-top: 1px solid var(--border);
}
.card__details {
  display: grid;
  gap: 0.35rem;
  margin: 0.75rem 0;
  font-size: 0.85rem;
}
.card__details div {
  display: grid;
  grid-template-columns: 4.5rem 1fr;
  gap: 0.5rem;
}
.card__details dt {
  color: var(--muted);
}
.card__details dd {
  margin: 0;
}
.card__loading,
.card__empty-body {
  color: var(--muted);
  font-size: 0.9rem;
}
.card__error {
  display: grid;
  gap: 0.5rem;
  justify-items: start;
  color: var(--danger);
}
.card__actions {
  display: flex;
  gap: 0.35rem;
  margin-top: 0.75rem;
}
</style>
