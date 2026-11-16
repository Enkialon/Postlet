<script setup lang="ts">
import { computed, ref } from 'vue';
import type { MessageWithRecipients } from '../../domain/entities';
import {
  addressHoverTitle,
  displayNameOrAddress,
  formatChatTime,
} from '../../shared/formatting/format';
import Avatar from '../../shared/ui/Avatar.vue';
import ContextMenu, {
  type ContextMenuItem,
} from '../../shared/ui/ContextMenu.vue';

const props = defineProps<{
  item: MessageWithRecipients;
  selected?: boolean;
  /** 会话联系人昵称；邮件 from_name 缺失或等于邮箱时回退 */
  preferredName?: string | null;
}>();

const emit = defineEmits<{
  select: [];
  reply: [];
  markUnread: [];
  delete: [];
  reassign: [messageId: string];
}>();

const menuOpen = ref(false);
const menuX = ref(0);
const menuY = ref(0);

const menuItems = computed<ContextMenuItem[]>(() => {
  const items: ContextMenuItem[] = [
    { id: 'open', label: '查看邮件' },
    { id: 'reply', label: '回复' },
    {
      id: 'markUnread',
      label: '标记未读',
      disabled: !props.item.message.is_read,
    },
    { id: 'reassign', label: '改到其他会话' },
  ];
  if (props.item.message.sync_status !== 'deleted_remote') {
    items.push({ id: 'delete', label: '删除邮件', danger: true });
  }
  return items;
});

/** 头像旁：有昵称用昵称，没有才用邮箱 */
const senderLabel = computed(() => {
  if (props.item.message.is_outbound) return '我';
  return (
    displayNameOrAddress(
      props.item.message.from_name,
      props.item.message.from_address,
      props.preferredName,
    ) || '未知'
  );
});

const senderTitle = computed(() => {
  if (props.item.message.is_outbound) {
    return props.item.message.from_address?.trim() || '我';
  }
  return (
    addressHoverTitle(
      props.item.message.from_name,
      props.item.message.from_address,
      props.preferredName,
    ) || senderLabel.value
  );
});

const timeLabel = computed(() =>
  formatChatTime(
    props.item.message.received_at ?? props.item.message.sent_at,
  ),
);

const previewText = computed(
  () => props.item.message.body_preview?.trim() || '',
);

const outbound = computed(() => props.item.message.is_outbound);

function openMenu(e: MouseEvent): void {
  e.preventDefault();
  menuX.value = e.clientX;
  menuY.value = e.clientY;
  menuOpen.value = true;
}

function onMenuSelect(id: string): void {
  menuOpen.value = false;
  if (id === 'open') {
    emit('select');
    return;
  }
  if (id === 'reply') {
    emit('reply');
    return;
  }
  if (id === 'markUnread') {
    emit('markUnread');
    return;
  }
  if (id === 'delete') {
    emit('delete');
    return;
  }
  if (id === 'reassign') {
    emit('reassign', props.item.message.id);
  }
}
</script>

<template>
  <article
    class="flex w-full min-w-0 max-w-full items-start gap-1.5 @[28rem]/chat:gap-2.5"
    :class="outbound ? 'ml-auto flex-row-reverse' : ''"
    @contextmenu="openMenu"
  >
    <Avatar
      :name="senderLabel"
      :kind="outbound ? 'self' : 'contact'"
      :size="36"
      class="mt-0.5 shrink-0"
    />

    <!-- 相对 @container/chat：窄栏近满宽换行，宽栏再抬到 360/420 上限 -->
    <div
      class="grid min-w-0 w-fit max-w-[calc(100%-2.75rem)] gap-1 @[28rem]/chat:max-w-[min(360px,calc(100%-2.75rem))] @[40rem]/chat:max-w-[min(420px,calc(100%-2.75rem))] @[28rem]/chat:gap-1.5"
      :class="outbound ? 'justify-items-end' : 'justify-items-start'"
    >
      <div
        class="flex max-w-full min-w-0 items-center gap-2 px-0.5"
        :class="outbound ? 'flex-row-reverse' : ''"
      >
        <span
          class="min-w-0 truncate text-[12px] font-medium text-text-secondary"
          :title="senderTitle"
          v-no-i18n
        >
          {{ senderLabel }}
        </span>
        <time
          class="shrink-0 text-[11px] tabular-nums text-text-tertiary"
        >
          {{ timeLabel }}
        </time>
        <span
          v-if="!item.message.is_read && !outbound"
          class="size-1.5 shrink-0 rounded-full bg-unread"
          aria-label="未读"
        />
      </div>

      <button
        type="button"
        class="box-border w-fit min-w-0 max-w-full cursor-pointer overflow-hidden rounded-[12px] px-3 py-2 text-left transition-[box-shadow,background-color] duration-150 @[28rem]/chat:rounded-[14px] @[28rem]/chat:px-3.5 @[28rem]/chat:py-2.5"
        :class="[
          outbound
            ? 'rounded-tr-md bg-bubble-outgoing'
            : 'rounded-tl-md bg-bubble-incoming shadow-xs',
          selected
            ? 'ring-2 ring-brand/30 ring-offset-1 ring-offset-surface-chat'
            : '',
        ]"
        :aria-pressed="selected"
        @click="emit('select')"
        @contextmenu="openMenu"
      >
        <p
          v-if="item.message.subject"
          class="m-0 text-[13px] font-medium leading-snug break-words whitespace-normal text-text-primary"
          :title="item.message.subject"
          v-no-i18n
        >
          {{ item.message.subject }}
        </p>

        <p
          v-if="previewText"
          class="m-0 line-clamp-4 text-[13px] leading-[1.5] break-words whitespace-pre-wrap text-text-secondary"
          :class="item.message.subject ? 'mt-1' : ''"
          v-no-i18n
        >
          {{ previewText }}
        </p>

        <p
          v-else-if="!item.message.subject"
          class="m-0 text-[13px] text-text-tertiary"
        >
          （无正文预览）
        </p>

        <p
          v-if="
            item.message.has_attachments ||
            item.message.sync_status === 'pending_local' ||
            item.message.sync_status === 'deleted_remote'
          "
          class="mt-1.5 mb-0 flex gap-2 text-[11px] text-text-tertiary"
        >
          <span v-if="item.message.has_attachments">含附件</span>
          <span v-if="item.message.sync_status === 'pending_local'"
            >发送中</span
          >
          <span
            v-if="item.message.sync_status === 'deleted_remote'"
            class="text-danger"
            >已删除</span
          >
        </p>
      </button>
    </div>

    <ContextMenu
      :open="menuOpen"
      :x="menuX"
      :y="menuY"
      :items="menuItems"
      @close="menuOpen = false"
      @select="onMenuSelect"
    />
  </article>
</template>
