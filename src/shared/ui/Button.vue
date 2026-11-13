<script setup lang="ts">
import { computed } from 'vue';

const props = withDefaults(
  defineProps<{
    variant?: 'primary' | 'secondary' | 'ghost' | 'danger';
    disabled?: boolean;
    type?: 'button' | 'submit';
    ariaLabel?: string;
  }>(),
  {
    variant: 'secondary',
    disabled: false,
    type: 'button',
  },
);

const classes = computed(() => {
  const base =
    'rounded-md border border-transparent px-3.5 py-1.5 text-[13px] font-medium leading-snug transition-colors duration-150 disabled:cursor-not-allowed disabled:opacity-45';
  switch (props.variant) {
    case 'primary':
      return `${base} bg-brand text-white hover:enabled:bg-brand-hover`;
    case 'danger':
      return `${base} bg-danger text-white`;
    case 'ghost':
      return `${base} bg-transparent text-text-secondary hover:enabled:bg-surface-muted hover:enabled:text-text-primary`;
    default:
      return `${base} border-border-subtle bg-surface-elevated text-text-primary hover:enabled:bg-surface-muted`;
  }
});
</script>

<template>
  <button
    :class="classes"
    :type="type"
    :disabled="disabled"
    :aria-label="ariaLabel"
  >
    <slot />
  </button>
</template>
