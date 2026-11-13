<script setup lang="ts">
import { onMounted, onUnmounted, ref, watch } from 'vue';
import ErrorState from '../../shared/ui/ErrorState.vue';
import Button from '../../shared/ui/Button.vue';
import { useAccounts } from './useAccounts';

const props = defineProps<{
  open: boolean;
}>();
const emit = defineEmits<{
  close: [];
}>();

const accounts = useAccounts();
const panelRef = ref<HTMLElement | null>(null);
let previousFocus: HTMLElement | null = null;

function onKey(e: KeyboardEvent) {
  if (!props.open) return;
  if (e.key === 'Escape' && !accounts.signingIn.value) {
    e.preventDefault();
    emit('close');
  }
}

watch(
  () => props.open,
  (open) => {
    if (open) {
      accounts.error.value = null;
      previousFocus = document.activeElement as HTMLElement | null;
      requestAnimationFrame(() => panelRef.value?.focus());
    } else if (previousFocus) {
      previousFocus.focus();
      previousFocus = null;
    }
  },
);

onMounted(() => window.addEventListener('keydown', onKey));
onUnmounted(() => window.removeEventListener('keydown', onKey));

async function loginOutlook(): Promise<void> {
  await accounts.signIn();
  if (!accounts.error.value) {
    emit('close');
  }
}

function close(): void {
  if (accounts.signingIn.value) return;
  emit('close');
}
</script>

<template>
  <div
    v-if="open"
    class="fixed inset-0 z-[60] grid place-items-center"
    role="presentation"
  >
    <div class="absolute inset-0 bg-black/30" @click="close" />
    <div
      ref="panelRef"
      class="relative w-[min(400px,calc(100vw-2rem))] rounded-lg border border-border-subtle bg-surface-elevated px-5 py-5 shadow-panel"
      role="dialog"
      aria-modal="true"
      aria-labelledby="add-account-title"
      tabindex="-1"
    >
      <header class="mb-1 flex items-start justify-between gap-3">
        <div>
          <h2
            id="add-account-title"
            class="m-0 text-base font-semibold text-text-primary"
          >
            添加账号
          </h2>
          <p class="mt-1 mb-0 text-[13px] leading-normal text-text-secondary">
            选择登录方式
          </p>
        </div>
        <button
          type="button"
          class="grid size-8 shrink-0 cursor-pointer place-items-center rounded-md border-0 bg-transparent text-text-tertiary transition-colors hover:bg-surface-muted hover:text-text-primary disabled:cursor-not-allowed disabled:opacity-45"
          aria-label="关闭"
          :disabled="accounts.signingIn.value"
          @click="close"
        >
          <svg class="size-4" viewBox="0 0 16 16" aria-hidden="true">
            <path
              fill="currentColor"
              d="M3.2 3.2a.75.75 0 0 1 1.06 0L8 6.94l3.74-3.74a.75.75 0 1 1 1.06 1.06L9.06 8l3.74 3.74a.75.75 0 1 1-1.06 1.06L8 9.06l-3.74 3.74a.75.75 0 1 1-1.06-1.06L6.94 8 3.2 4.26a.75.75 0 0 1 0-1.06Z"
            />
          </svg>
        </button>
      </header>

      <div class="mt-4 grid gap-2" role="list">
        <button
          type="button"
          role="listitem"
          class="flex w-full cursor-pointer items-center gap-3 rounded-md border border-border-subtle bg-surface-elevated px-3 py-3 text-left transition-colors duration-150 hover:bg-surface-muted disabled:cursor-not-allowed disabled:opacity-55"
          :disabled="accounts.signingIn.value"
          aria-label="使用 Outlook 登录"
          @click="loginOutlook"
        >
          <span
            class="grid size-10 shrink-0 place-items-center rounded-md bg-[#0078D4] text-white"
            aria-hidden="true"
          >
            <svg class="size-5" viewBox="0 0 24 24">
              <path
                fill="currentColor"
                d="M21.5 6.4v11.2c0 .9-.7 1.6-1.6 1.6H9.2V4.8h10.7c.9 0 1.6.7 1.6 1.6ZM3.4 5.9l5 2.1v8.2l-5 2.1A1.3 1.3 0 0 1 1.5 17V7a1.3 1.3 0 0 1 1.9-1.1Zm10.9 2.6c-2.1 0-3.8 1.4-3.8 3.5s1.7 3.5 3.8 3.5 3.8-1.4 3.8-3.5-1.7-3.5-3.8-3.5Zm0 1.5c1.2 0 2.1.8 2.1 2s-.9 2-2.1 2-2.1-.8-2.1-2 .9-2 2.1-2Z"
              />
            </svg>
          </span>
          <span class="min-w-0 flex-1">
            <span class="block text-[14px] font-medium text-text-primary">
              Outlook
            </span>
            <span
              v-if="accounts.signingIn.value"
              class="block text-[12px] text-text-tertiary"
            >
              正在打开浏览器，请完成登录…
            </span>
          </span>
        </button>
      </div>

      <ErrorState
        v-if="accounts.error.value"
        class="mt-3! p-0!"
        :error="accounts.error.value"
        @retry="loginOutlook"
      />

      <footer class="mt-4 flex justify-end">
        <Button
          variant="ghost"
          :disabled="accounts.signingIn.value"
          @click="close"
        >
          取消
        </Button>
      </footer>
    </div>
  </div>
</template>
