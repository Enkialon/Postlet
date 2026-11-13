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

const COMPOSE_MAX_RATIO = 0.75;

/** 快捷写信高度（可拖、持久化，随阅读区高度等比上限） */
export function useComposePaneHeight(rootEl: Ref<HTMLElement | null>) {
  const prefs = usePreferences();
  const parentH = ref(640);
  let ro: ResizeObserver | null = null;

  function maxAllowed(): number {
    return Math.min(
      prefs.limits.compose.max,
      Math.floor(parentH.value * COMPOSE_MAX_RATIO),
    );
  }

  function minAllowed(): number {
    return prefs.limits.compose.min;
  }

  const heightPx = computed(() => {
    const preferred = prefs.preferences.panelWidths.compose;
    const cap = maxAllowed();
    const floor = minAllowed();
    return Math.min(Math.max(preferred, floor), Math.max(floor, cap));
  });

  const compactStyle = computed(() => ({
    height: `${heightPx.value}px`,
  }));

  const { dragging, startDrag } = usePaneDrag({
    axis: 'y',
    sign: -1,
    getValue: () => heightPx.value,
    setValue: (n) => prefs.setPanelWidths({ compose: n }),
    min: minAllowed,
    max: maxAllowed,
    cursor: 'row-resize',
  });

  function observeRoot(el: HTMLElement | null): void {
    ro?.disconnect();
    ro = null;
    const parent = el?.parentElement;
    if (!parent) return;
    const sync = () => {
      parentH.value = parent.clientHeight || 640;
    };
    sync();
    ro = new ResizeObserver(sync);
    ro.observe(parent);
  }

  onMounted(() => observeRoot(rootEl.value));
  watch(rootEl, (el) => observeRoot(el));
  onUnmounted(() => ro?.disconnect());

  return {
    heightPx,
    compactStyle,
    dragging,
    startDrag,
  };
}
