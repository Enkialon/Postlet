<script setup lang="ts">
defineProps<{ title: string; description?: string }>();
</script>

<template>
  <div class="empty" role="status">
    <p class="empty__title">{{ title }}</p>
    <p v-if="description" class="empty__desc">{{ description }}</p>
    <div v-if="$slots.default" class="empty__actions">
      <slot />
    </div>
  </div>
</template>

<style scoped>
.empty {
  padding: 2rem 1.25rem;
  text-align: center;
  color: var(--muted);
}
.empty__title {
  margin: 0;
  color: var(--text);
  font-size: 1rem;
}
.empty__desc {
  margin: 0.5rem 0 0;
  font-size: 0.9rem;
}
.empty__actions {
  margin-top: 1rem;
}
</style>
