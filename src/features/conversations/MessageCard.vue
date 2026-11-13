<script setup lang="ts">
import { computed, ref } from 'vue';
import type { MessageWithRecipients } from '../../domain/entities';
import {
  displayNameOrAddress,
  formatTimestampOrUnknown,
} from '../../shared/formatting/format';
import Avatar from '../../shared/ui/Avatar.vue';
import ContextMenu, {
  type ContextMenuItem,
} from '../../shared/ui/ContextMenu.vue';

const props = defineProps<{
  item: MessageWithRecipients;
  selected?: boolean;
  allowMoveThread?: boolean;
}>();

const emit = defineEmits<{
  select: [];
  reply: [];
  markUnread: [];
  moveThread: [threadId: string];
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
  ];
  if (props.allowMoveThread && props.item.message.thread_id) {
    items.push({ id: 'moveThread', label: '迁移到其他群组' });
  }
  return items;
});

function fromLabel(): string {
  return (
    displayNameOrAddress(
      props.item.message.from_name,
      props.item.message.from_address,
    ) || '未知'
  );
}

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
  if (id === 'moveThread' && props.item.message.thread_id) {
    emit('moveThread', props.item.message.thread_id);
  }
}
</script>

<template>
  <article
    class="flex max-w-[820px] items-start gap-3"
    :class="item.message.is_outbound ? 'ml-auto flex-row justify-end' : ''"
    @contextmenu="openMenu"
  >
    <Avatar
      v-if="!item.message.is_outbound"
      :name="fromLabel()"
      :size="36"
    />

    <div
      class="grid max-w-[min(560px,78vw)] min-w-0 gap-1.5"
      :class="item.message.is_outbound ? 'justify-items-end' : ''"
    >
      <div
        class="flex items-center gap-2 px-0.5 text-[12px] text-text-tertiary"
        :class="item.message.is_outbound ? 'justify-end' : ''"
      >
        <span
          v-if="!item.message.is_outbound"
          class="text-[13px] font-medium text-text-secondary"
          v-no-i18n
        >
          {{ fromLabel() }}
        </span>
        <time>
          {{
            formatTimestampOrUnknown(
              item.message.received_at ?? item.message.sent_at,
            )
          }}
        </time>
        <span
          v-if="!item.message.is_read"
          class="font-medium text-unread"
          aria-label="未读"
          >未读</span
        >
      </div>

      <button
        type="button"
        class="relative max-w-full cursor-pointer rounded-lg border px-3.5 py-2.5 text-left text-text-primary transition-colors duration-150"
        :class="[
          item.message.is_outbound
            ? 'border-border-bubble-out bg-bubble-outgoing'
            : 'border-border-bubble bg-bubble-incoming',
          selected ? 'ring-1 ring-brand/35' : '',
        ]"
        :aria-pressed="selected"
        @click="emit('select')"
        @contextmenu="openMenu"
      >
        <strong
          class="mb-1 block text-[14px] font-semibold text-text-primary"
          v-no-i18n
          >{{ item.message.subject }}</strong
        >
        <p
          class="m-0 line-clamp-3 text-[14px] leading-[1.55] break-words whitespace-pre-wrap text-text-primary"
          v-no-i18n
        >
          {{ item.message.body_preview }}
        </p>
        <p
          v-if="item.message.has_attachments"
          class="mt-2 mb-0 text-[12px] text-text-tertiary"
        >
          含附件
        </p>
        <p
          v-if="item.message.sync_status === 'pending_local'"
          class="mt-2 mb-0 text-[12px] text-text-tertiary"
        >
          等待同步
        </p>
      </button>
    </div>

    <Avatar
      v-if="item.message.is_outbound"
      name="我"
      kind="self"
      :size="36"
    />

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
