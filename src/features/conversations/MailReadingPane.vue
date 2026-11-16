<script setup lang="ts">
import { computed, ref } from 'vue';
import type { MessageWithRecipients } from '../../domain/entities';
import type { RecipientRole } from '../../domain/types';
import {
  displayNameOrAddress,
  formatTimestampOrUnknown,
} from '../../shared/formatting/format';
import Avatar from '../../shared/ui/Avatar.vue';
import Button from '../../shared/ui/Button.vue';
import OverlayScrollArea from '../../shared/ui/OverlayScrollArea.vue';
import MailBodyView from '../../shared/mail-content/MailBodyView.vue';

const props = defineProps<{
  item: MessageWithRecipients;
  bodyLoading: boolean;
  bodyError: string | null;
  /** 会话联系人昵称；from_name 缺失或等于邮箱时回退 */
  preferredName?: string | null;
}>();

const emit = defineEmits<{
  close: [];
  retryBody: [];
  delete: [];
}>();

const showMoreMeta = ref(false);

const fromLabel = computed(() => {
  if (props.item.message.is_outbound) return '我';
  return (
    displayNameOrAddress(
      props.item.message.from_name,
      props.item.message.from_address,
      props.preferredName,
    ) || '未知'
  );
});

function recipientsByRole(role: RecipientRole): string {
  return props.item.recipients
    .filter((r) => r.role === role)
    .sort((a, b) => a.seq - b.seq)
    .map((r) => displayNameOrAddress(r.display_name, r.normalized_address))
    .filter(Boolean)
    .join('、');
}

const toLine = computed(() => recipientsByRole('to'));
const ccLine = computed(() => recipientsByRole('cc'));
const bccLine = computed(() => recipientsByRole('bcc'));

const timeLabel = computed(() =>
  formatTimestampOrUnknown(
    props.item.message.received_at ?? props.item.message.sent_at,
  ),
);

const importanceLabel = computed(() => {
  switch (props.item.message.importance) {
    case 'high':
      return '高';
    case 'low':
      return '低';
    default:
      return '普通';
  }
});

const syncLabel = computed(() => {
  switch (props.item.message.sync_status) {
    case 'pending_local':
      return '等待同步';
    case 'deleted_remote':
      return '远端已删除';
    default:
      return '已同步';
  }
});

const folderLabel = computed(
  () =>
    props.item.message.parent_folder_name ||
    props.item.message.folder_id ||
    null,
);
</script>

<template>
  <div
    class="flex h-full min-h-0 flex-col bg-surface-elevated"
    aria-label="邮件阅读"
  >
    <OverlayScrollArea class="min-h-0 flex-1" type="hover">
      <div class="bg-surface-elevated px-5 pt-1.5 pb-5">
        <div class="mb-2.5 flex items-start gap-2">
          <h1
            class="mt-0 mb-0 min-w-0 flex-1 text-[16px] leading-snug font-semibold text-text-primary"
            v-no-i18n
          >
            {{ item.message.subject || '（无主题）' }}
          </h1>
          <div class="flex shrink-0 items-center gap-1">
            <Button
              v-if="item.message.sync_status !== 'deleted_remote'"
              variant="ghost"
              class="!px-2 shrink-0 text-danger"
              aria-label="删除邮件"
              @click="emit('delete')"
            >
              删除
            </Button>
            <span
              v-else
              class="px-2 text-[12px] text-danger"
            >已删除</span>
            <Button
              variant="ghost"
              class="!px-2 shrink-0"
              aria-label="关闭"
              @click="emit('close')"
            >
              ✕
            </Button>
          </div>
        </div>

        <div class="flex items-start gap-3">
          <Avatar
            :name="fromLabel"
            :kind="item.message.is_outbound ? 'self' : 'contact'"
            :size="40"
          />
          <div class="grid min-w-0 flex-1 gap-1">
            <div class="flex items-baseline justify-between gap-3">
              <strong
                class="text-[14px] font-semibold text-text-primary"
                v-no-i18n
                >{{ fromLabel }}</strong
              >
              <time class="shrink-0 text-[12px] text-text-tertiary">{{
                timeLabel
              }}</time>
            </div>
            <p
              v-if="toLine"
              class="m-0 text-[13px] leading-normal text-text-secondary"
              v-no-i18n
            >
              <span class="mr-2 inline-block min-w-[3em] text-text-tertiary"
                >收件人</span
              >{{ toLine }}
            </p>
            <p
              v-if="ccLine"
              class="m-0 text-[13px] leading-normal text-text-secondary"
              v-no-i18n
            >
              <span class="mr-2 inline-block min-w-[3em] text-text-tertiary"
                >抄送</span
              >{{ ccLine }}
            </p>
            <p
              v-if="bccLine"
              class="m-0 text-[13px] leading-normal text-text-secondary"
              v-no-i18n
            >
              <span class="mr-2 inline-block min-w-[3em] text-text-tertiary"
                >密送</span
              >{{ bccLine }}
            </p>
            <button
              type="button"
              class="mt-1 justify-self-start border-0 bg-transparent p-0 text-[13px] font-medium text-brand transition-opacity duration-150 hover:opacity-80"
              @click="showMoreMeta = !showMoreMeta"
            >
              {{ showMoreMeta ? '收起详情' : '更多详情' }}
            </button>
            <div v-if="showMoreMeta" class="mt-2 flex flex-wrap gap-1.5">
              <span
                v-if="folderLabel"
                class="rounded-pill bg-surface-muted px-2.5 py-1 text-[12px] text-text-secondary"
                v-no-i18n
                >{{ folderLabel }}</span
              >
              <span
                class="rounded-pill bg-surface-muted px-2.5 py-1 text-[12px] text-text-secondary"
                >重要性 · {{ importanceLabel }}</span
              >
              <span
                class="rounded-pill bg-surface-muted px-2.5 py-1 text-[12px] text-text-secondary"
                >{{ item.message.has_attachments ? '有附件' : '无附件' }}</span
              >
              <span
                class="rounded-pill bg-surface-muted px-2.5 py-1 text-[12px] text-text-secondary"
                >{{ syncLabel }}</span
              >
            </div>
          </div>
        </div>

        <div class="my-5 h-px bg-border-subtle" />

        <div class="min-h-48">
          <div
            v-if="bodyLoading"
            class="mt-6 text-[14px] text-text-tertiary"
            aria-live="polite"
          >
            正在加载正文…
          </div>
          <div
            v-else-if="bodyError"
            class="mt-4 grid justify-items-start gap-2 text-danger"
          >
            <p class="m-0 text-[14px]">{{ bodyError }}</p>
            <Button
              v-if="bodyError !== '邮件已不可用'"
              @click="emit('retryBody')"
            >
              重试
            </Button>
          </div>
          <p
            v-else-if="item.message.body_content === ''"
            class="mt-6 text-[14px] text-text-tertiary"
          >
            此邮件正文为空
          </p>
          <div
            v-else-if="item.message.body_content != null"
            class="py-1 text-text-primary"
          >
            <MailBodyView
              :content="item.message.body_content"
              :content-type="item.message.body_content_type"
              :title="item.message.subject || '邮件正文'"
            />
          </div>
          <p v-else class="mt-6 text-[14px] text-text-tertiary">正文尚未加载</p>
        </div>
      </div>
    </OverlayScrollArea>
  </div>
</template>
