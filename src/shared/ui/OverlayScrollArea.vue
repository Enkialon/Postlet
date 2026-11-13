<script setup lang="ts">
import { computed, useTemplateRef } from 'vue';
import {
  ScrollAreaCorner,
  ScrollAreaRoot,
  ScrollAreaScrollbar,
  ScrollAreaThumb,
  ScrollAreaViewport,
} from 'reka-ui';

/** 与 reka-ui ScrollAreaRoot type 对齐（包入口未导出 ScrollType） */
type OverlayScrollType = 'auto' | 'always' | 'scroll' | 'hover' | 'glimpse';

const props = withDefaults(
  defineProps<{
    /** hover：悬停显示；scroll：滚动时显示；always：始终显示；glimpse：短暂闪现 */
    type?: OverlayScrollType;
    class?: string;
  }>(),
  { type: 'hover' },
);

/** ScrollAreaViewport 暴露 viewportElement（Ref，经实例访问会 unwrap） */
const viewportCompRef = useTemplateRef<{
  viewportElement: HTMLElement | null | undefined;
}>('viewportComp');

const viewport = computed(
  () => viewportCompRef.value?.viewportElement ?? null,
);

function getViewport(): HTMLElement | null {
  return viewport.value;
}

defineExpose({ getViewport, viewport });
</script>

<template>
  <ScrollAreaRoot
    class="relative h-full min-h-0 w-full overflow-hidden"
    :class="props.class"
    :type="props.type"
    :scroll-hide-delay="600"
  >
    <ScrollAreaViewport
      ref="viewportComp"
      class="h-full w-full rounded-[inherit]"
    >
      <slot />
    </ScrollAreaViewport>
    <ScrollAreaScrollbar
      class="z-10 flex touch-none select-none bg-transparent transition-opacity duration-150 data-[orientation=vertical]:w-1.5 data-[orientation=horizontal]:h-1.5 data-[orientation=horizontal]:flex-col"
      orientation="vertical"
    >
      <ScrollAreaThumb
        class="relative flex-1 rounded-pill bg-black/40 before:absolute before:left-1/2 before:top-1/2 before:h-full before:min-h-[44px] before:w-full before:min-w-[44px] before:-translate-x-1/2 before:-translate-y-1/2 before:content-[''] hover:bg-black/55"
      />
    </ScrollAreaScrollbar>
    <ScrollAreaCorner />
  </ScrollAreaRoot>
</template>
