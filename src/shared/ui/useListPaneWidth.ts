import { computed, onMounted, onUnmounted, ref } from 'vue';
import { usePreferences } from '../../app/preferences';

const RAIL_PX = 56;
/** 列表栏相对内容区最大占比，窗口缩小时等比收窄 */
const LIST_MAX_RATIO = 0.38;
const MIN_LIST = 200;
const MAX_LIST = 520;

/** 会话 / 联系人 / 群组共用列表栏宽度（可拖、持久化，随窗口等比上限） */
export function useListPaneWidth() {
  const prefs = usePreferences();
  const dragging = ref(false);
  const viewportW = ref(
    typeof window !== 'undefined' ? window.innerWidth : 1200,
  );

  function contentWidth(): number {
    return Math.max(320, viewportW.value - RAIL_PX);
  }

  function maxAllowed(): number {
    return Math.min(MAX_LIST, Math.floor(contentWidth() * LIST_MAX_RATIO));
  }

  const widthPx = computed(() => {
    const preferred = prefs.preferences.panelWidths.list;
    const cap = maxAllowed();
    // 窗口变窄时按占比上限等比收缩，不低于 MIN
    return clamp(Math.min(preferred, cap), MIN_LIST, Math.max(MIN_LIST, cap));
  });

  const listStyle = computed(() => ({
    width: `${widthPx.value}px`,
  }));

  let startX = 0;
  let startW = 0;

  function onPointerMove(e: PointerEvent): void {
    if (!dragging.value) return;
    const cap = maxAllowed();
    const next = clamp(startW + (e.clientX - startX), MIN_LIST, cap);
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

  function onWinResize(): void {
    viewportW.value = window.innerWidth;
  }

  onMounted(() => {
    window.addEventListener('resize', onWinResize);
  });

  onUnmounted(() => {
    window.removeEventListener('resize', onWinResize);
    stopDrag();
  });

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
