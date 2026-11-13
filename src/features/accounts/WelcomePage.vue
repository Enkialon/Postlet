<script setup lang="ts">
import Button from '../../shared/ui/Button.vue';
import ErrorState from '../../shared/ui/ErrorState.vue';
import { useAccounts } from './useAccounts';

const { signingIn, error, signIn, loadDemoWorkspace } = useAccounts();
</script>

<template>
  <section class="grid min-h-full place-items-center bg-surface-chat p-8">
    <div
      class="grid w-[min(420px,100%)] justify-items-start gap-3 rounded-lg border border-border-subtle bg-surface-elevated p-6"
    >
      <p class="m-0 text-[28px] font-semibold tracking-tight text-brand">
        Postlet
      </p>
      <h1 class="m-0 text-base font-semibold text-text-primary">
        像微信一样看邮件往来
      </h1>
      <p class="m-0 text-[14px] leading-normal text-text-secondary">
        以联系人与会话为中心。先用演示数据把业务跑通，再接 Outlook。
      </p>
      <div class="mt-1 flex flex-wrap gap-2">
        <Button
          variant="primary"
          :disabled="signingIn"
          aria-label="加载演示数据"
          @click="loadDemoWorkspace"
        >
          {{ signingIn ? '正在加载…' : '加载演示数据' }}
        </Button>
        <Button
          :disabled="signingIn"
          aria-label="登录空账户"
          @click="signIn"
        >
          登录空账户
        </Button>
      </div>
      <p class="m-0 text-[13px] leading-normal text-text-tertiary">
        演示模式不连邮箱，适合验收会话、联系人、群组与同步交互。
      </p>
      <ErrorState
        v-if="error"
        :error="error"
        @retry="loadDemoWorkspace"
      />
    </div>
  </section>
</template>
