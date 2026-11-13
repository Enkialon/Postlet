<script setup lang="ts">
import { onMounted, onUnmounted, ref, watch } from 'vue';
import Button from './Button.vue';

const props = defineProps<{
  open: boolean;
  title: string;
  confirmLabel?: string;
  cancelLabel?: string;
  confirming?: boolean;
  danger?: boolean;
}>();
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
  <div v-if="open" class="dialog-root" role="presentation">
    <div class="dialog-backdrop" @click="emit('close')" />
    <div
      ref="panelRef"
      class="dialog"
      role="dialog"
      aria-modal="true"
      :aria-label="title"
      tabindex="-1"
    >
      <header class="dialog__header">
        <h2>{{ title }}</h2>
      </header>
      <div class="dialog__body">
        <slot />
      </div>
      <footer class="dialog__footer">
        <Button variant="ghost" :disabled="confirming" @click="emit('close')">
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

<style scoped>
.dialog-root {
  position: fixed;
  inset: 0;
  z-index: 50;
  display: grid;
  place-items: center;
}
.dialog-backdrop {
  position: absolute;
  inset: 0;
  background: rgba(20, 24, 28, 0.45);
}
.dialog {
  position: relative;
  width: min(440px, calc(100vw - 2rem));
  background: var(--surface);
  border: 1px solid var(--border);
  border-radius: 10px;
  padding: 1rem 1.1rem;
  box-shadow: 0 12px 40px rgba(0, 0, 0, 0.18);
}
.dialog__header h2 {
  margin: 0;
  font-size: 1.05rem;
}
.dialog__body {
  margin: 0.85rem 0 1rem;
  color: var(--text);
  font-size: 0.95rem;
}
.dialog__footer {
  display: flex;
  justify-content: flex-end;
  gap: 0.5rem;
}
</style>
