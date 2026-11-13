<script setup lang="ts">
import { computed } from 'vue';

const props = withDefaults(
  defineProps<{
    name: string;
    kind?: 'contact' | 'group' | 'self';
    size?: number;
  }>(),
  {
    kind: 'contact',
    size: 40,
  },
);

/** 高明度、中高饱和；深色字保证可读 */
const palette = [
  { bg: '#D4F0E2', fg: '#2F7A55' },
  { bg: '#D6E6FB', fg: '#2F5FA0' },
  { bg: '#FBE2C8', fg: '#A05A1E' },
  { bg: '#E4F0C8', fg: '#5A7A1E' },
  { bg: '#E6D9FB', fg: '#5B3FA0' },
  { bg: '#FBD5D8', fg: '#A03A48' },
  { bg: '#D0F0EE', fg: '#1E7A72' },
  { bg: '#FBDCC8', fg: '#A05528' },
];

const groupTone = { bg: '#D8E4F0', fg: '#3A5A7A' };
const selfTone = { bg: '#CDEBD9', fg: '#2F7A55' };

const initial = computed(() => {
  const t = props.name.trim();
  if (!t) return props.kind === 'group' ? '群' : '?';
  if (props.kind === 'group') return '群';
  return t.slice(0, 1).toUpperCase();
});

const tone = computed(() => {
  if (props.kind === 'group') return groupTone;
  if (props.kind === 'self') return selfTone;
  let hash = 0;
  for (const ch of props.name) hash = (hash * 31 + ch.charCodeAt(0)) >>> 0;
  return palette[hash % palette.length]!;
});

const style = computed(() => ({
  width: `${props.size}px`,
  height: `${props.size}px`,
  fontSize: `${Math.max(12, Math.round(props.size * 0.36))}px`,
  background: tone.value.bg,
  color: tone.value.fg,
}));
</script>

<template>
  <span
    class="inline-grid shrink-0 place-items-center rounded-md font-semibold select-none"
    :style="style"
    aria-hidden="true"
  >{{ initial }}</span>
</template>
