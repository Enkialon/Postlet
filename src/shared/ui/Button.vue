<script setup lang="ts">
withDefaults(
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
</script>

<template>
  <button
    class="btn"
    :class="`btn--${variant}`"
    :type="type"
    :disabled="disabled"
    :aria-label="ariaLabel"
  >
    <slot />
  </button>
</template>

<style scoped>
.btn {
  border-radius: 6px;
  border: 1px solid transparent;
  padding: 0.45rem 0.85rem;
  font: inherit;
  font-size: 0.9rem;
  cursor: pointer;
  line-height: 1.3;
}
.btn:disabled {
  opacity: 0.55;
  cursor: not-allowed;
}
.btn--primary {
  background: var(--accent);
  color: #fff;
}
.btn--secondary {
  background: var(--surface-2);
  color: var(--text);
  border-color: var(--border);
}
.btn--ghost {
  background: transparent;
  color: var(--text);
}
.btn--danger {
  background: var(--danger);
  color: #fff;
}
.btn:focus-visible {
  outline: 2px solid var(--accent);
  outline-offset: 2px;
}
</style>
