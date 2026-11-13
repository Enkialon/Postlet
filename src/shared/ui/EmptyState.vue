<script setup lang="ts">
defineProps<{ title: string; description?: string }>();
</script>

<template>
  <div class="px-4 py-8 text-center" role="status">
    <p class="m-0 text-[14px] font-medium text-text-secondary">{{ title }}</p>
    <p
      v-if="description"
      class="mt-2 mb-0 text-[13px] leading-normal text-text-tertiary"
    >
      {{ description }}
    </p>
    <div v-if="$slots.default" class="mt-4">
      <slot />
    </div>
  </div>
</template>
