<script setup lang="ts">
import { ref } from 'vue';
import Button from '../../shared/ui/Button.vue';
import ErrorState from '../../shared/ui/ErrorState.vue';
import { useAccounts } from './useAccounts';
import AddAccountDialog from './AddAccountDialog.vue';

const { signingIn, error, loadDemoWorkspace } = useAccounts();
const addAccountOpen = ref(false);
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
        以联系人与会话为中心。可登录 Outlook（Graph）或 IMAP/SMTP 邮箱，也可先加载演示数据。
      </p>
      <div class="mt-1 flex flex-wrap gap-2">
        <Button
          variant="primary"
          :disabled="signingIn"
          aria-label="登录账号"
          @click="addAccountOpen = true"
        >
          登录账号
        </Button>
        <Button
          :disabled="signingIn"
          aria-label="加载演示数据"
          @click="loadDemoWorkspace"
        >
          {{ signingIn ? '正在加载…' : '加载演示数据' }}
        </Button>
      </div>
      <p class="m-0 text-[13px] leading-normal text-text-tertiary">
        Outlook 走浏览器 OAuth；IMAP/SMTP 在对话框内填写主机与密码。演示模式不连邮箱。
      </p>
      <ErrorState
        v-if="error && !addAccountOpen"
        :error="error"
        @retry="addAccountOpen = true"
      />
    </div>

    <AddAccountDialog
      :open="addAccountOpen"
      @close="addAccountOpen = false"
    />
  </section>
</template>
