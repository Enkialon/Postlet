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
    class="h-full max-w-[640px] overflow-auto bg-surface-chat px-6 py-5 pb-8"
    aria-label="设置"
  >
    <h1 class="mb-4 mt-0 text-base font-semibold text-text-primary">设置</h1>

    <section
      class="mb-3 rounded-lg border border-border-subtle bg-surface-elevated px-4 py-3.5"
    >
      <h2 class="mb-2.5 mt-0 text-[13px] font-medium text-text-secondary">
        当前账户
      </h2>
      <template v-if="accounts.current.value">
        <p v-no-i18n class="m-0 text-[14px] font-semibold text-text-primary">
          {{ accounts.current.value.display_name }}
        </p>
        <p v-no-i18n class="mt-1 mb-3 text-[13px] text-text-secondary">
          {{ accounts.current.value.email }}
        </p>
        <div class="flex flex-wrap gap-2">
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
        <p class="text-[14px] text-text-secondary">无账户</p>
        <Button
          variant="primary"
          :disabled="accounts.signingIn.value"
          @click="accounts.loadDemoWorkspace"
        >
          加载演示数据
        </Button>
      </template>
    </section>

    <section
      class="mb-3 rounded-lg border border-border-subtle bg-surface-elevated px-4 py-3.5"
    >
      <h2 class="mb-2.5 mt-0 text-[13px] font-medium text-text-secondary">
        显示
      </h2>
      <label class="flex items-start gap-2 text-[14px] leading-normal">
        <input
          type="checkbox"
          :checked="
            accounts.current.value
              ?.show_group_messages_in_contact_conversation ?? false
          "
          :disabled="saving || !accounts.current.value"
          @change="onShowGroupChange"
        />
        在联系人会话中同时显示群组邮件
      </label>
      <p v-if="error" class="text-[13px] text-danger">{{ error }}</p>
    </section>

    <section
      class="mb-3 rounded-lg border border-border-subtle bg-surface-elevated px-4 py-3.5"
    >
      <h2 class="mb-2.5 mt-0 text-[13px] font-medium text-text-secondary">
        语言
      </h2>
      <label class="text-[14px]">
        界面语言
        <select
          class="mt-1.5 block rounded-md border border-border-subtle bg-surface-input px-2.5 py-2 text-[14px]"
          :value="prefs.preferences.locale"
          @change="onLocaleChange"
        >
          <option value="zh-CN">中文</option>
          <option value="en-US">English</option>
        </select>
      </label>
    </section>

    <section
      class="mb-3 rounded-lg border border-border-subtle bg-surface-elevated px-4 py-3.5"
    >
      <h2 class="mb-2.5 mt-0 text-[13px] font-medium text-text-secondary">
        同步
      </h2>
      <p class="text-[14px] text-text-primary">{{ sync.mailStatusLabel() }}</p>
      <p
        v-if="sync.mailProgress.value?.mail_sync_error"
        class="text-[13px] text-danger"
      >
        {{ sync.mailProgress.value.mail_sync_error }}
      </p>
      <p
        v-if="sync.contactTriggerHint.value"
        class="text-[13px] text-text-secondary"
      >
        {{ sync.contactTriggerHint.value }}
      </p>
      <p
        v-if="sync.contactProgress.value?.contact_synced_at"
        class="text-[13px] text-text-secondary"
      >
        联系人最近同步于
        {{ formatTimestamp(sync.contactProgress.value.contact_synced_at) }}
      </p>
      <div class="mt-2.5 flex flex-wrap gap-2">
        <Button @click="sync.triggerMail">同步邮件</Button>
        <Button @click="sync.triggerContacts">同步联系人</Button>
      </div>

      <div v-if="sync.mailProgress.value?.folders?.length">
        <h3 class="mb-1.5 mt-3.5 text-[14px] font-medium text-text-primary">
          文件夹同步状态
        </h3>
        <ul class="m-0 pl-4 text-[13px] text-text-secondary">
          <li
            v-for="f in sync.mailProgress.value.folders"
            :key="f.folder_id"
          >
            <span v-no-i18n>{{ f.folder_id }}</span>
            —
            {{
              f.sync_status === 'running'
                ? '正在同步'
                : f.sync_status === 'error'
                  ? '部分内容可能未更新'
                  : f.synced_at
                    ? formatTimestamp(f.synced_at)
                    : '尚未完成首次同步'
            }}
          </li>
        </ul>
      </div>
    </section>
  </section>
</template>
