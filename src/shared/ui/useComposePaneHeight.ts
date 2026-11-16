import {
  computed,
  onMounted,
  onUnmounted,
  ref,
  watch,
  type Ref,
} from 'vue';
import { usePreferences } from '../../app/preferences';
import { usePaneDrag } from './usePaneDrag';

/** 相对阅读区高度的上限占比；过大时矮窗口几乎不收缩 */
const COMPOSE_MAX_RATIO = 0.45;
const ABSOLUTE_FLOOR = 120;

/** 快捷写信高度（可拖、持久化，随阅读区高度等比收缩） */
export function useComposePaneHeight(rootEl: Ref<HTMLElement | null>) {
  const prefs = usePreferences();
  const parentH = ref(640);
  let ro: ResizeObserver | null = null;

  function bounds(): { min: number; max: number } {
    const hardMin = prefs.limits.compose.min;
    const hardMax = prefs.limits.compose.max;
    const byRatio = Math.floor(parentH.value * COMPOSE_MAX_RATIO);
    let max = Math.min(hardMax, Math.max(ABSOLUTE_FLOOR, byRatio));
    let min = hardMin;
    // 矮窗口：硬下限压不过上限时，把下限降到可放得下
    if (max < min) {
      min = Math.max(ABSOLUTE_FLOOR, max);
      max = Math.max(max, min);
    }
    return { min, max };
  }

  const heightPx = computed(() => {
    const preferred = prefs.preferences.panelWidths.compose;
    const { min, max } = bounds();
    // preferred 超出当前窗口上限时按上限显示，不改持久化值
    return Math.min(Math.max(Math.min(preferred, max), min), max);
  });

  const compactStyle = computed(() => ({
    height: `${heightPx.value}px`,
  }));

  const { dragging, startDrag } = usePaneDrag({
    axis: 'y',
    sign: -1,
    getValue: () => heightPx.value,
    setValue: (n) => prefs.setPanelWidths({ compose: n }),
    min: () => bounds().min,
    max: () => bounds().max,
    cursor: 'row-resize',
  });

  function syncParentHeight(parent: HTMLElement): void {
    const next = parent.clientHeight;
    if (next > 0) parentH.value = next;
  }

  function observeRoot(el: HTMLElement | null): void {
    ro?.disconnect();
    ro = null;
    const parent = el?.parentElement;
    if (!parent) return;
    syncParentHeight(parent);
    ro = new ResizeObserver(() => syncParentHeight(parent));
    ro.observe(parent);
  }

  onMounted(() => {
    observeRoot(rootEl.value);
    // 首帧布局完成后再读一次，避免 mount 时 parent 高度仍为 0
    requestAnimationFrame(() => {
      const parent = rootEl.value?.parentElement;
      if (parent) syncParentHeight(parent);
    });
  });
  watch(rootEl, (el) => observeRoot(el));
  onUnmounted(() => ro?.disconnect());

  return {
    heightPx,
    compactStyle,
    dragging,
    startDrag,
  };
}
