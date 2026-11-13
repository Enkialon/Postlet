<script setup lang="ts">
import { nextTick, onMounted, onUnmounted, ref, watch } from 'vue';

export interface ContextMenuItem {
  id: string;
  label: string;
  disabled?: boolean;
  danger?: boolean;
}

const props = defineProps<{
  open: boolean;
  x: number;
  y: number;
  items: ContextMenuItem[];
}>();

const emit = defineEmits<{
  close: [];
  select: [id: string];
}>();

const menuRef = ref<HTMLElement | null>(null);
const pos = ref({ left: 0, top: 0 });

function place(): void {
  const el = menuRef.value;
  if (!el) return;
  const pad = 8;
  const rect = el.getBoundingClientRect();
  const maxX = window.innerWidth - rect.width - pad;
  const maxY = window.innerHeight - rect.height - pad;
  pos.value = {
    left: Math.max(pad, Math.min(props.x, maxX)),
    top: Math.max(pad, Math.min(props.y, maxY)),
  };
}

watch(
  () => [props.open, props.x, props.y, props.items.length] as const,
  async ([open]) => {
    if (!open) return;
    await nextTick();
    place();
  },
);

function onDocPointer(e: PointerEvent): void {
  if (!props.open) return;
  const t = e.target as Node | null;
  if (menuRef.value && t && menuRef.value.contains(t)) return;
  emit('close');
}

function onKey(e: KeyboardEvent): void {
  if (!props.open) return;
  if (e.key === 'Escape') {
    e.preventDefault();
    emit('close');
  }
}

onMounted(() => {
  window.addEventListener('pointerdown', onDocPointer, true);
  window.addEventListener('keydown', onKey);
  window.addEventListener('resize', () => emit('close'));
  window.addEventListener('scroll', () => emit('close'), true);
});
onUnmounted(() => {
  window.removeEventListener('pointerdown', onDocPointer, true);
  window.removeEventListener('keydown', onKey);
});
</script>

<template>
  <Teleport to="body">
    <div
      v-if="open"
      ref="menuRef"
      class="fixed z-[80] min-w-[148px] rounded-lg border border-border-subtle bg-surface-elevated py-1 shadow-panel"
      role="menu"
      :style="{ left: `${pos.left}px`, top: `${pos.top}px` }"
    >
      <button
        v-for="item in items"
        :key="item.id"
        type="button"
        role="menuitem"
        class="block w-full cursor-pointer border-0 bg-transparent px-3.5 py-2 text-left text-[13px] font-medium transition-colors duration-150 hover:enabled:bg-surface-muted disabled:cursor-not-allowed disabled:opacity-45"
        :class="item.danger ? 'text-danger' : 'text-text-primary'"
        :disabled="item.disabled"
        @click="emit('select', item.id)"
      >
        {{ item.label }}
      </button>
    </div>
  </Teleport>
</template>
