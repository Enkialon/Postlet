<script setup lang="ts">
defineProps<{
  dragging?: boolean;
}>();

const emit = defineEmits<{
  start: [event: PointerEvent];
}>();
</script>

<template>
  <div
    role="separator"
    aria-orientation="vertical"
    aria-label="调整列表宽度"
    tabindex="0"
    class="group relative z-[2] w-0 shrink-0 cursor-col-resize touch-none max-[760px]:hidden"
    :class="dragging ? 'bg-brand/25' : ''"
    @pointerdown="emit('start', $event)"
  >
    <span
      class="absolute inset-y-0 -left-1 w-2 transition-colors duration-150 group-hover:bg-brand/15 group-active:bg-brand/25"
      :class="dragging ? 'bg-brand/20' : ''"
      aria-hidden="true"
    />
  </div>
</template>
