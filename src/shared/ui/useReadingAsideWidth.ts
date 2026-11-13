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

const MAIL_MAX_RATIO = 0.9;
/** 阅读区去掉列表后往往偏窄，详情需更高占比才拖得动 */
const DETAIL_MAX_RATIO = 0.82;

/** 读邮件 / 会话详情侧栏宽度（可拖、持久化，随容器等比上限） */
export function useReadingAsideWidth(
  paneEl: Ref<HTMLElement | null>,
  getMode: () => 'mail' | 'detail',
) {
  const prefs = usePreferences();
  const containerW = ref(800);
  let ro: ResizeObserver | null = null;

  function hardMin(): number {
    return getMode() === 'mail'
      ? prefs.limits.mail.min
      : prefs.limits.detail.min;
  }

  function hardMax(): number {
    return getMode() === 'mail'
      ? prefs.limits.mail.max
      : prefs.limits.detail.max;
  }

  function ratio(): number {
    return getMode() === 'mail' ? MAIL_MAX_RATIO : DETAIL_MAX_RATIO;
  }

  /** 保证 max >= min，窄窗口时仍可微调 */
  function bounds(): { min: number; max: number } {
    const minHard = hardMin();
    const byRatio = Math.floor(containerW.value * ratio());
    let max = Math.min(hardMax(), byRatio);
    let min = minHard;
    if (max < min) {
      min = Math.max(160, max);
      max = Math.max(max, min);
    }
    return { min, max };
  }

  const widthPx = computed(() => {
    const preferred =
      getMode() === 'mail'
        ? prefs.preferences.panelWidths.mail
        : prefs.preferences.panelWidths.detail;
    const { min, max } = bounds();
    return Math.min(Math.max(preferred, min), max);
  });

  const asideStyle = computed(() => ({
    width: `${widthPx.value}px`,
  }));

  const { dragging, startDrag } = usePaneDrag({
    axis: 'x',
    sign: -1,
    getValue: () => widthPx.value,
    setValue: (n) => {
      if (getMode() === 'mail') prefs.setPanelWidths({ mail: n });
      else prefs.setPanelWidths({ detail: n });
    },
    min: () => bounds().min,
    max: () => bounds().max,
    cursor: 'col-resize',
  });

  function observe(el: HTMLElement | null): void {
    ro?.disconnect();
    ro = null;
    if (!el) return;
    const sync = () => {
      containerW.value = el.clientWidth || 800;
    };
    sync();
    ro = new ResizeObserver(sync);
    ro.observe(el);
  }

  onMounted(() => observe(paneEl.value));
  watch(paneEl, (el) => observe(el));
  onUnmounted(() => ro?.disconnect());

  return {
    widthPx,
    asideStyle,
    dragging,
    startDrag,
  };
}
