import { onUnmounted, ref } from 'vue';

export type PaneDragAxis = 'x' | 'y';

/**
 * 通用面板拖动：写入外部 setValue（通常进 preferences），松手即已持久化。
 * sign=-1：右栏左缘 / 底栏顶缘（指针往内拖变大）。
 */
export function usePaneDrag(options: {
  axis: PaneDragAxis;
  sign: 1 | -1;
  getValue: () => number;
  setValue: (n: number) => void;
  min: () => number;
  max: () => number;
  cursor: string;
}) {
  const dragging = ref(false);
  let startPointer = 0;
  let startValue = 0;

  function onPointerMove(e: PointerEvent): void {
    if (!dragging.value) return;
    const delta =
      options.axis === 'x'
        ? e.clientX - startPointer
        : e.clientY - startPointer;
    const next = clamp(
      startValue + options.sign * delta,
      options.min(),
      options.max(),
    );
    options.setValue(Math.round(next));
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
    e.stopPropagation();
    dragging.value = true;
    startPointer = options.axis === 'x' ? e.clientX : e.clientY;
    startValue = options.getValue();
    document.body.style.cursor = options.cursor;
    document.body.style.userSelect = 'none';
    window.addEventListener('pointermove', onPointerMove);
    window.addEventListener('pointerup', stopDrag);
    window.addEventListener('pointercancel', stopDrag);
  }

  onUnmounted(stopDrag);

  return { dragging, startDrag, stopDrag };
}

function clamp(n: number, min: number, max: number): number {
  return Math.min(max, Math.max(min, n));
}
