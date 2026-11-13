<script setup lang="ts">
import Button from '../../shared/ui/Button.vue';
import ErrorState from '../../shared/ui/ErrorState.vue';
import { useAccounts } from './useAccounts';

const { signingIn, error, signIn, loadDemoWorkspace } = useAccounts();
</script>

<template>
  <section class="welcome">
    <div class="welcome__panel">
      <p class="welcome__brand">Postlet</p>
      <h1>欢迎使用 Postlet</h1>
      <p class="welcome__desc">
        以联系人与会话为中心的 Outlook 邮件客户端。
      </p>
      <div class="welcome__actions">
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
      <p class="welcome__hint">
        当前未接 Outlook：演示数据可走完会话 / 联系人 / 群组业务；空账户用于空态验收。
      </p>
      <ErrorState
        v-if="error"
        :error="error"
        @retry="loadDemoWorkspace"
      />
    </div>
  </section>
</template>

<style scoped>
.welcome {
  min-height: 100%;
  display: grid;
  place-items: center;
  padding: 2rem;
  background:
    radial-gradient(ellipse at 20% 10%, #dce8f5 0%, transparent 50%),
    radial-gradient(ellipse at 80% 90%, #e8efe6 0%, transparent 45%),
    var(--bg);
}
.welcome__panel {
  width: min(420px, 100%);
  display: grid;
  gap: 0.85rem;
  justify-items: start;
}
.welcome__brand {
  margin: 0;
  font-family: "Segoe UI Semibold", "Source Han Sans SC", sans-serif;
  font-size: 2rem;
  letter-spacing: -0.03em;
  color: var(--accent);
}
h1 {
  margin: 0;
  font-size: 1.35rem;
  font-weight: 600;
}
.welcome__desc {
  margin: 0 0 0.5rem;
  color: var(--muted);
  line-height: 1.5;
}
.welcome__actions {
  display: flex;
  flex-wrap: wrap;
  gap: 0.5rem;
}
.welcome__hint {
  margin: 0;
  font-size: 0.85rem;
  color: var(--muted);
  line-height: 1.45;
}
</style>
