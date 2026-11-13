<script setup lang="ts">
import { onMounted, onUnmounted, ref, watch } from 'vue';
import Button from './Button.vue';

const props = withDefaults(
  defineProps<{
    open: boolean;
    title: string;
    confirmLabel?: string;
    cancelLabel?: string;
    confirming?: boolean;
    danger?: boolean;
    showCancel?: boolean;
  }>(),
  {
    showCancel: true,
  },
);
const emit = defineEmits<{
  close: [];
  confirm: [];
}>();

const panelRef = ref<HTMLElement | null>(null);
let previousFocus: HTMLElement | null = null;

function onKey(e: KeyboardEvent) {
  if (!props.open) return;
  if (e.key === 'Escape') {
    e.preventDefault();
    emit('close');
  }
}

watch(
  () => props.open,
  (open) => {
    if (open) {
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
</script>

<template>
  <div
    v-if="open"
    class="fixed inset-0 z-50 grid place-items-center"
    role="presentation"
  >
    <div class="absolute inset-0 bg-black/30" @click="emit('close')" />
    <div
      ref="panelRef"
      class="relative w-[min(420px,calc(100vw-2rem))] rounded-lg border border-border-subtle bg-surface-elevated px-4 py-4 shadow-panel"
      role="dialog"
      aria-modal="true"
      :aria-label="title"
      tabindex="-1"
    >
      <header>
        <h2 class="m-0 text-base font-semibold text-text-primary">
          {{ title }}
        </h2>
      </header>
      <div class="my-3 mb-4 text-[14px] leading-normal text-text-primary">
        <slot />
      </div>
      <footer class="flex justify-end gap-2">
        <Button
          v-if="showCancel"
          variant="ghost"
          :disabled="confirming"
          @click="emit('close')"
        >
          {{ cancelLabel || '取消' }}
        </Button>
        <Button
          :variant="danger ? 'danger' : 'primary'"
          :disabled="confirming"
          @click="emit('confirm')"
        >
          {{ confirmLabel || '确认' }}
        </Button>
      </footer>
    </div>
  </div>
</template>
