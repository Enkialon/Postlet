<script setup lang="ts">
import type { AppError } from '../../domain/errors';
import { errorMessageForUi } from '../errors/asAppError';
import Button from './Button.vue';

const props = defineProps<{
  error: AppError;
  staleHint?: boolean;
}>();
const emit = defineEmits<{ retry: [] }>();
</script>

<template>
  <div class="error" role="alert">
    <p class="error__msg">{{ errorMessageForUi(props.error) }}</p>
    <p v-if="staleHint" class="error__hint">内容可能未更新</p>
    <p v-if="props.error.message" class="error__detail">
      {{ props.error.message }}
    </p>
    <Button
      v-if="props.error.retryable"
      variant="secondary"
      @click="emit('retry')"
    >
      重试
    </Button>
  </div>
</template>

<style scoped>
.error {
  padding: 1rem;
  display: grid;
  gap: 0.5rem;
  justify-items: start;
}
.error__msg {
  margin: 0;
  color: var(--danger);
  font-weight: 600;
}
.error__hint,
.error__detail {
  margin: 0;
  color: var(--muted);
  font-size: 0.85rem;
}
</style>
