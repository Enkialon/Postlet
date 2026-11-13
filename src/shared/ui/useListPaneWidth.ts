import { computed, onUnmounted, ref } from 'vue';
import { usePreferences } from '../../app/preferences';

const MIN_LIST = 220;
const MAX_LIST = 520;

/** 会话 / 联系人 / 群组共用列表栏宽度（可拖、持久化） */
export function useListPaneWidth() {
  const prefs = usePreferences();
  const dragging = ref(false);

  const widthPx = computed(() =>
    clamp(prefs.preferences.panelWidths.list, MIN_LIST, MAX_LIST),
  );

  const listStyle = computed(() => ({
    width: `${widthPx.value}px`,
  }));

  let startX = 0;
  let startW = 0;

  function onPointerMove(e: PointerEvent): void {
    if (!dragging.value) return;
    const next = clamp(startW + (e.clientX - startX), MIN_LIST, MAX_LIST);
    prefs.setPanelWidths({ list: Math.round(next) });
  }

  function stopDrag(): void {
    if (!dragging.value) return;
    dragging.value = false;
    window.removeEventListener('pointermove', onPointerMove);
    window.removeEventListener('pointerup', stopDrag);
    window.removeEventListener('pointercancel', stopDrag);
    document.body.style.cursor = '';
    document.body.style.userSelect = '';
  }

  function startDrag(e: PointerEvent): void {
    if (e.button !== 0) return;
    e.preventDefault();
    dragging.value = true;
    startX = e.clientX;
    startW = widthPx.value;
    document.body.style.cursor = 'col-resize';
    document.body.style.userSelect = 'none';
    window.addEventListener('pointermove', onPointerMove);
    window.addEventListener('pointerup', stopDrag);
    window.addEventListener('pointercancel', stopDrag);
  }

  onUnmounted(stopDrag);

  return {
    widthPx,
    listStyle,
    dragging,
    startDrag,
    min: MIN_LIST,
    max: MAX_LIST,
  };
}

function clamp(n: number, min: number, max: number): number {
  return Math.min(max, Math.max(min, n));
}
