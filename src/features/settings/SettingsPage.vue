<script setup lang="ts">
import { useSettings } from './useSettings';
import { useSyncStatus } from '../sync/useSyncStatus';
import { formatTimestamp } from '../../shared/formatting/format';
import Button from '../../shared/ui/Button.vue';
import type { AppLocale } from '../../i18n';

const { accounts, prefs, saving, error, setShowGroupInContact, setLocale } =
  useSettings();
const sync = useSyncStatus();

function onLocaleChange(e: Event) {
  setLocale((e.target as HTMLSelectElement).value as AppLocale);
}

function onShowGroupChange(e: Event) {
  void setShowGroupInContact((e.target as HTMLInputElement).checked);
}
</script>

<template>
  <section
    class="flex h-full min-h-0 w-full flex-col bg-surface-chat"
    aria-label="设置"
  >
    <header
      class="shrink-0 border-b border-border-subtle bg-surface-header px-8 py-5"
    >
      <h1 class="m-0 text-xl font-semibold tracking-tight text-text-primary">
        设置
      </h1>
      <p class="mt-1 mb-0 text-[13px] text-text-secondary">
        账户、显示与同步偏好
      </p>
    </header>

    <div class="min-h-0 flex-1 overflow-auto">
      <form
        class="mx-auto flex w-full max-w-2xl flex-col gap-8 px-8 py-8"
        @submit.prevent
      >
        <!-- 账户 -->
        <fieldset class="m-0 min-w-0 border-0 p-0">
          <legend
            class="mb-3 w-full border-b border-border-subtle pb-2 text-[13px] font-semibold tracking-wide text-text-secondary uppercase"
          >
            当前账户
          </legend>
          <template v-if="accounts.current.value">
            <div class="flex flex-col gap-1">
              <p
                v-no-i18n
                class="m-0 text-[15px] font-semibold text-text-primary"
              >
                {{ accounts.current.value.display_name }}
              </p>
              <p v-no-i18n class="m-0 text-[13px] text-text-secondary">
                {{ accounts.current.value.email }}
              </p>
            </div>
            <div class="mt-4 flex flex-wrap gap-2">
              <Button variant="danger" @click="accounts.signOut">退出</Button>
              <Button
                :disabled="accounts.signingIn.value"
                @click="accounts.loadDemoWorkspace"
              >
                恢复演示数据
              </Button>
            </div>
          </template>
          <template v-else>
            <p class="m-0 text-[14px] text-text-secondary">无账户</p>
            <div class="mt-4">
              <Button
                variant="primary"
                :disabled="accounts.signingIn.value"
                @click="accounts.loadDemoWorkspace"
              >
                加载演示数据
              </Button>
            </div>
          </template>
        </fieldset>

        <!-- 显示 -->
        <fieldset class="m-0 min-w-0 border-0 p-0">
          <legend
            class="mb-3 w-full border-b border-border-subtle pb-2 text-[13px] font-semibold tracking-wide text-text-secondary uppercase"
          >
            显示
          </legend>
          <label
            class="flex cursor-pointer items-start gap-3 text-[14px] leading-snug text-text-primary"
          >
            <input
              class="mt-0.5"
              type="checkbox"
              :checked="
                accounts.current.value
                  ?.show_group_messages_in_contact_conversation ?? false
              "
              :disabled="saving || !accounts.current.value"
              @change="onShowGroupChange"
            />
            <span>
              在联系人会话中同时显示群组邮件
              <span class="mt-1 block text-[12px] text-text-tertiary">
                关闭时，群组邮件只出现在群组会话中
              </span>
            </span>
          </label>
          <p v-if="error" class="mt-2 mb-0 text-[13px] text-danger">
            {{ error }}
          </p>
        </fieldset>

        <!-- 语言 -->
        <fieldset class="m-0 min-w-0 border-0 p-0">
          <legend
            class="mb-3 w-full border-b border-border-subtle pb-2 text-[13px] font-semibold tracking-wide text-text-secondary uppercase"
          >
            语言
          </legend>
          <label class="flex flex-col gap-1.5 text-[14px] text-text-primary">
            界面语言
            <select
              class="w-full max-w-xs rounded-md border border-border-subtle bg-surface-input px-3 py-2 text-[14px] text-text-primary outline-none focus:border-brand"
              :value="prefs.preferences.locale"
              @change="onLocaleChange"
            >
              <option value="zh-CN">中文</option>
              <option value="en-US">English</option>
            </select>
          </label>
        </fieldset>

        <!-- 同步 -->
        <fieldset class="m-0 min-w-0 border-0 p-0">
          <legend
            class="mb-3 w-full border-b border-border-subtle pb-2 text-[13px] font-semibold tracking-wide text-text-secondary uppercase"
          >
            同步
          </legend>
          <div class="flex flex-col gap-2 text-[14px]">
            <p class="m-0 text-text-primary">{{ sync.mailStatusLabel() }}</p>
            <p
              v-if="sync.mailProgress.value?.mail_sync_error"
              class="m-0 text-[13px] text-danger"
            >
              {{ sync.mailProgress.value.mail_sync_error }}
            </p>
            <p
              v-if="sync.contactTriggerHint.value"
              class="m-0 text-[13px] text-text-secondary"
            >
              {{ sync.contactTriggerHint.value }}
            </p>
            <p
              v-if="sync.contactProgress.value?.contact_synced_at"
              class="m-0 text-[13px] text-text-secondary"
            >
              联系人最近同步于
              {{ formatTimestamp(sync.contactProgress.value.contact_synced_at) }}
            </p>
          </div>
          <div class="mt-4 flex flex-wrap gap-2">
            <Button @click="sync.triggerMail">同步邮件</Button>
            <Button @click="sync.triggerContacts">同步联系人</Button>
          </div>

          <div
            v-if="sync.mailProgress.value?.folders?.length"
            class="mt-6"
          >
            <h3
              class="mb-2 mt-0 text-[13px] font-medium text-text-secondary"
            >
              文件夹同步状态
            </h3>
            <ul
              class="m-0 list-none divide-y divide-border-subtle rounded-md border border-border-subtle bg-surface-elevated p-0"
            >
              <li
                v-for="f in sync.mailProgress.value.folders"
                :key="f.folder_id"
                class="flex flex-wrap items-baseline justify-between gap-2 px-3 py-2.5 text-[13px]"
              >
                <span v-no-i18n class="font-medium text-text-primary">{{
                  f.folder_id
                }}</span>
                <span class="text-text-secondary">
                  {{
                    f.sync_status === 'running'
                      ? '正在同步'
                      : f.sync_status === 'error'
                        ? '部分内容可能未更新'
                        : f.synced_at
                          ? formatTimestamp(f.synced_at)
                          : '尚未完成首次同步'
                  }}
                </span>
              </li>
            </ul>
          </div>
        </fieldset>
      </form>
    </div>
  </section>
</template>
