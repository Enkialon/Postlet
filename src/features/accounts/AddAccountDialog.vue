<script setup lang="ts">
import { onMounted, onUnmounted, reactive, ref, watch } from 'vue';
import ErrorState from '../../shared/ui/ErrorState.vue';
import Button from '../../shared/ui/Button.vue';
import type { MailTlsMode } from '../../domain/types';
import { useAccounts } from './useAccounts';

type Step = 'providers' | 'imap';

const props = defineProps<{
  open: boolean;
}>();
const emit = defineEmits<{
  close: [];
}>();

const accounts = useAccounts();
const panelRef = ref<HTMLElement | null>(null);
const step = ref<Step>('providers');
let previousFocus: HTMLElement | null = null;

const form = reactive({
  email: '',
  display_name: '',
  username: '',
  password: '',
  imap_host: '',
  imap_port: 993,
  imap_tls: 'implicit' as MailTlsMode,
  smtp_host: '',
  smtp_port: 587,
  smtp_tls: 'starttls' as MailTlsMode,
});

const fieldClass =
  'w-full rounded-md border border-border-subtle bg-surface-input px-3 py-2 text-[14px] text-text-primary outline-none focus:border-brand disabled:opacity-55';

function resetForm(): void {
  form.email = '';
  form.display_name = '';
  form.username = '';
  form.password = '';
  form.imap_host = '';
  form.imap_port = 993;
  form.imap_tls = 'implicit';
  form.smtp_host = '';
  form.smtp_port = 587;
  form.smtp_tls = 'starttls';
}

function onKey(e: KeyboardEvent) {
  if (!props.open) return;
  if (e.key === 'Escape' && !accounts.signingIn.value) {
    e.preventDefault();
    if (step.value === 'imap') {
      step.value = 'providers';
      accounts.error.value = null;
      return;
    }
    emit('close');
  }
}

watch(
  () => props.open,
  (open) => {
    if (open) {
      accounts.error.value = null;
      step.value = 'providers';
      resetForm();
      previousFocus = document.activeElement as HTMLElement | null;
      requestAnimationFrame(() => panelRef.value?.focus());
    } else if (previousFocus) {
      previousFocus.focus();
      previousFocus = null;
    }
  },
);

onMounted(() => window.addEventListener('keydown', onKey));
onUnmounted(() => window.removeEventListener('keydown', onKey));

async function loginOutlook(): Promise<void> {
  await accounts.signIn();
  if (!accounts.error.value) {
    emit('close');
  }
}

async function submitImap(): Promise<void> {
  const email = form.email.trim();
  const imapHost = form.imap_host.trim();
  const smtpHost = form.smtp_host.trim();
  if (!email || !imapHost || !smtpHost || !form.password) {
    accounts.error.value = {
      code: 'validation',
      message: '请填写邮箱、IMAP/SMTP 主机与密码',
      retryable: false,
    };
    return;
  }
  await accounts.addImapSmtpAccount({
    email,
    display_name: form.display_name.trim() || email,
    auth_kind: 'password',
    imap_host: imapHost,
    imap_port: Number(form.imap_port) || 993,
    imap_tls: form.imap_tls,
    smtp_host: smtpHost,
    smtp_port: Number(form.smtp_port) || 587,
    smtp_tls: form.smtp_tls,
    username: form.username.trim() || email,
    password: form.password,
    access_token: null,
    refresh_token: null,
    expires_at_ms: null,
  });
  if (!accounts.error.value) {
    emit('close');
  }
}

function close(): void {
  if (accounts.signingIn.value) return;
  emit('close');
}

function backToProviders(): void {
  if (accounts.signingIn.value) return;
  step.value = 'providers';
  accounts.error.value = null;
}
</script>

<template>
  <div
    v-if="open"
    class="fixed inset-0 z-[60] grid place-items-center"
    role="presentation"
  >
    <div class="absolute inset-0 bg-black/30" @click="close" />
    <div
      ref="panelRef"
      class="relative w-[min(440px,calc(100vw-2rem))] rounded-lg border border-border-subtle bg-surface-elevated px-5 py-5 shadow-panel"
      role="dialog"
      aria-modal="true"
      aria-labelledby="add-account-title"
      tabindex="-1"
    >
      <header class="mb-1 flex items-start justify-between gap-3">
        <div>
          <h2
            id="add-account-title"
            class="m-0 text-base font-semibold text-text-primary"
          >
            {{ step === 'providers' ? '添加账号' : 'IMAP / SMTP' }}
          </h2>
          <p class="mt-1 mb-0 text-[13px] leading-normal text-text-secondary">
            {{
              step === 'providers'
                ? '选择登录方式'
                : '按收发协议配置，不绑定具体邮箱品牌'
            }}
          </p>
        </div>
        <button
          type="button"
          class="grid size-8 shrink-0 cursor-pointer place-items-center rounded-md border-0 bg-transparent text-text-tertiary transition-colors hover:bg-surface-muted hover:text-text-primary disabled:cursor-not-allowed disabled:opacity-45"
          aria-label="关闭"
          :disabled="accounts.signingIn.value"
          @click="close"
        >
          <svg class="size-4" viewBox="0 0 16 16" aria-hidden="true">
            <path
              fill="currentColor"
              d="M3.2 3.2a.75.75 0 0 1 1.06 0L8 6.94l3.74-3.74a.75.75 0 1 1 1.06 1.06L9.06 8l3.74 3.74a.75.75 0 1 1-1.06 1.06L8 9.06l-3.74 3.74a.75.75 0 1 1-1.06-1.06L6.94 8 3.2 4.26a.75.75 0 0 1 0-1.06Z"
            />
          </svg>
        </button>
      </header>

      <div v-if="step === 'providers'" class="mt-4 grid gap-2" role="list">
        <button
          type="button"
          role="listitem"
          class="flex w-full cursor-pointer items-center gap-3 rounded-md border border-border-subtle bg-surface-elevated px-3 py-3 text-left transition-colors duration-150 hover:bg-surface-muted disabled:cursor-not-allowed disabled:opacity-55"
          :disabled="accounts.signingIn.value"
          aria-label="使用 Outlook 登录"
          @click="loginOutlook"
        >
          <span
            class="grid size-10 shrink-0 place-items-center rounded-md bg-[#0078D4] text-white"
            aria-hidden="true"
          >
            <svg class="size-5" viewBox="0 0 24 24">
              <path
                fill="currentColor"
                d="M21.5 6.4v11.2c0 .9-.7 1.6-1.6 1.6H9.2V4.8h10.7c.9 0 1.6.7 1.6 1.6ZM3.4 5.9l5 2.1v8.2l-5 2.1A1.3 1.3 0 0 1 1.5 17V7a1.3 1.3 0 0 1 1.9-1.1Zm10.9 2.6c-2.1 0-3.8 1.4-3.8 3.5s1.7 3.5 3.8 3.5 3.8-1.4 3.8-3.5-1.7-3.5-3.8-3.5Zm0 1.5c1.2 0 2.1.8 2.1 2s-.9 2-2.1 2-2.1-.8-2.1-2 .9-2 2.1-2Z"
              />
            </svg>
          </span>
          <span class="min-w-0 flex-1">
            <span class="block text-[14px] font-medium text-text-primary">
              Outlook
            </span>
            <span
              v-if="accounts.signingIn.value"
              class="block text-[12px] text-text-tertiary"
            >
              正在打开浏览器，请完成登录…
            </span>
            <span v-else class="block text-[12px] text-text-tertiary">
              Microsoft Graph OAuth
            </span>
          </span>
        </button>

        <button
          type="button"
          role="listitem"
          class="flex w-full cursor-pointer items-center gap-3 rounded-md border border-border-subtle bg-surface-elevated px-3 py-3 text-left transition-colors duration-150 hover:bg-surface-muted disabled:cursor-not-allowed disabled:opacity-55"
          :disabled="accounts.signingIn.value"
          aria-label="使用 IMAP 与 SMTP 登录"
          @click="step = 'imap'"
        >
          <span
            class="grid size-10 shrink-0 place-items-center rounded-md bg-surface-muted text-text-primary"
            aria-hidden="true"
          >
            <svg class="size-5" viewBox="0 0 24 24">
              <path
                fill="currentColor"
                d="M4 6.5A2.5 2.5 0 0 1 6.5 4h11A2.5 2.5 0 0 1 20 6.5v11a2.5 2.5 0 0 1-2.5 2.5h-11A2.5 2.5 0 0 1 4 17.5v-11Zm2.5-.5a.5.5 0 0 0-.5.5v.3l6 3.75 6-3.75V6.5a.5.5 0 0 0-.5-.5h-11Zm11.5 3.2-5.55 3.47a.75.75 0 0 1-.8 0L6 9.2V17.5a.5.5 0 0 0 .5.5h11a.5.5 0 0 0 .5-.5V9.2Z"
              />
            </svg>
          </span>
          <span class="min-w-0 flex-1">
            <span class="block text-[14px] font-medium text-text-primary">
              IMAP / SMTP
            </span>
            <span class="block text-[12px] text-text-tertiary">
              通用邮箱，密码登录
            </span>
          </span>
        </button>
      </div>

      <form
        v-else
        class="mt-4 grid max-h-[min(70vh,560px)] gap-3 overflow-y-auto pr-0.5"
        @submit.prevent="submitImap"
      >
        <label class="grid gap-1.5 text-[13px] text-text-secondary">
          邮箱
          <input
            v-model="form.email"
            :class="fieldClass"
            type="email"
            autocomplete="username"
            required
            :disabled="accounts.signingIn.value"
          />
        </label>
        <label class="grid gap-1.5 text-[13px] text-text-secondary">
          显示名
          <input
            v-model="form.display_name"
            :class="fieldClass"
            type="text"
            autocomplete="name"
            :disabled="accounts.signingIn.value"
            placeholder="可选，默认用邮箱"
          />
        </label>
        <label class="grid gap-1.5 text-[13px] text-text-secondary">
          用户名
          <input
            v-model="form.username"
            :class="fieldClass"
            type="text"
            autocomplete="username"
            :disabled="accounts.signingIn.value"
            placeholder="可选，默认用邮箱"
          />
        </label>
        <label class="grid gap-1.5 text-[13px] text-text-secondary">
          密码
          <input
            v-model="form.password"
            :class="fieldClass"
            type="password"
            autocomplete="current-password"
            required
            :disabled="accounts.signingIn.value"
          />
        </label>

        <div class="grid grid-cols-2 gap-2">
          <label class="grid gap-1.5 text-[13px] text-text-secondary">
            IMAP 主机
            <input
              v-model="form.imap_host"
              :class="fieldClass"
              type="text"
              required
              :disabled="accounts.signingIn.value"
              placeholder="imap.example.com"
            />
          </label>
          <label class="grid gap-1.5 text-[13px] text-text-secondary">
            端口
            <input
              v-model.number="form.imap_port"
              :class="fieldClass"
              type="number"
              min="1"
              max="65535"
              required
              :disabled="accounts.signingIn.value"
            />
          </label>
        </div>
        <label class="grid gap-1.5 text-[13px] text-text-secondary">
          IMAP TLS
          <select
            v-model="form.imap_tls"
            :class="fieldClass"
            :disabled="accounts.signingIn.value"
          >
            <option value="implicit">隐式 TLS（如 993）</option>
            <option value="starttls">STARTTLS</option>
            <option value="plain">明文</option>
          </select>
        </label>

        <div class="grid grid-cols-2 gap-2">
          <label class="grid gap-1.5 text-[13px] text-text-secondary">
            SMTP 主机
            <input
              v-model="form.smtp_host"
              :class="fieldClass"
              type="text"
              required
              :disabled="accounts.signingIn.value"
              placeholder="smtp.example.com"
            />
          </label>
          <label class="grid gap-1.5 text-[13px] text-text-secondary">
            端口
            <input
              v-model.number="form.smtp_port"
              :class="fieldClass"
              type="number"
              min="1"
              max="65535"
              required
              :disabled="accounts.signingIn.value"
            />
          </label>
        </div>
        <label class="grid gap-1.5 text-[13px] text-text-secondary">
          SMTP TLS
          <select
            v-model="form.smtp_tls"
            :class="fieldClass"
            :disabled="accounts.signingIn.value"
          >
            <option value="starttls">STARTTLS（如 587）</option>
            <option value="implicit">隐式 TLS（如 465）</option>
            <option value="plain">明文</option>
          </select>
        </label>
      </form>

      <ErrorState
        v-if="accounts.error.value"
        class="mt-3! p-0!"
        :error="accounts.error.value"
        @retry="step === 'providers' ? loginOutlook() : submitImap()"
      />

      <footer class="mt-4 flex justify-end gap-2">
        <Button
          v-if="step === 'imap'"
          variant="ghost"
          :disabled="accounts.signingIn.value"
          @click="backToProviders"
        >
          返回
        </Button>
        <Button
          v-else
          variant="ghost"
          :disabled="accounts.signingIn.value"
          @click="close"
        >
          取消
        </Button>
        <Button
          v-if="step === 'imap'"
          variant="primary"
          :disabled="accounts.signingIn.value"
          @click="submitImap"
        >
          {{ accounts.signingIn.value ? '正在添加…' : '添加账号' }}
        </Button>
      </footer>
    </div>
  </div>
</template>
