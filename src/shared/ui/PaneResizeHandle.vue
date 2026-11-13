<script setup lang="ts">
withDefaults(
  defineProps<{
    /** vertical：左右分栏；horizontal：上下分栏 */
    orientation?: 'vertical' | 'horizontal';
    dragging?: boolean;
    label?: string;
  }>(),
  {
    orientation: 'vertical',
    dragging: false,
    label: '调整大小',
  },
);

const emit = defineEmits<{
  start: [event: PointerEvent];
}>();
</script>

<template>
  <div
    role="separator"
    :aria-orientation="orientation"
    :aria-label="label"
    tabindex="0"
    class="group z-[6] shrink-0 touch-none"
    :class="
      orientation === 'vertical'
        ? 'relative w-0 cursor-col-resize'
        : 'relative h-0 cursor-row-resize'
    "
    @pointerdown="emit('start', $event)"
  >
    <!-- 命中条落在容器内侧，避免被 overflow 裁切后点不到 -->
    <span
      class="absolute transition-colors duration-150 group-hover:bg-brand/15 group-active:bg-brand/25"
      :class="[
        dragging ? 'bg-brand/20' : '',
        orientation === 'vertical'
          ? 'inset-y-0 left-0 w-2'
          : 'inset-x-0 top-0 h-2',
      ]"
      aria-hidden="true"
    />
  </div>
</template>
