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
  <div class="grid justify-items-start gap-2 p-4" role="alert">
    <p class="m-0 text-[14px] font-medium text-danger">
      {{ errorMessageForUi(props.error) }}
    </p>
    <p v-if="staleHint" class="m-0 text-[13px] text-text-secondary">
      内容可能未更新
    </p>
    <p v-if="props.error.message" class="m-0 text-[13px] text-text-secondary">
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
