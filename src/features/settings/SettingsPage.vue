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
  <section class="settings" aria-label="设置">
    <h1>设置</h1>

    <section class="block">
      <h2>当前账户</h2>
      <template v-if="accounts.current.value">
        <p v-no-i18n class="name">{{ accounts.current.value.display_name }}</p>
        <p v-no-i18n class="email">{{ accounts.current.value.email }}</p>
        <div class="row">
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
        <p>无账户</p>
        <Button
          variant="primary"
          :disabled="accounts.signingIn.value"
          @click="accounts.loadDemoWorkspace"
        >
          加载演示数据
        </Button>
      </template>
    </section>

    <section class="block">
      <h2>显示</h2>
      <label class="check">
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
      <p v-if="error" class="err">{{ error }}</p>
    </section>

    <section class="block">
      <h2>语言</h2>
      <label>
        界面语言
        <select :value="prefs.preferences.locale" @change="onLocaleChange">
          <option value="zh-CN">中文</option>
          <option value="en-US">English</option>
        </select>
      </label>
    </section>

    <section class="block">
      <h2>同步</h2>
      <p>{{ sync.mailStatusLabel() }}</p>
      <p v-if="sync.mailProgress.value?.mail_sync_error" class="err">
        {{ sync.mailProgress.value.mail_sync_error }}
      </p>
      <p v-if="sync.contactTriggerHint.value">
        {{ sync.contactTriggerHint.value }}
      </p>
      <p v-if="sync.contactProgress.value?.contact_synced_at">
        联系人最近同步于
        {{ formatTimestamp(sync.contactProgress.value.contact_synced_at) }}
      </p>
      <div class="row">
        <Button @click="sync.triggerMail">同步邮件</Button>
        <Button @click="sync.triggerContacts">同步联系人</Button>
      </div>

      <div
        v-if="sync.mailProgress.value?.folders?.length"
        class="folders"
      >
        <h3>文件夹同步状态</h3>
        <ul>
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

<style scoped>
.settings {
  padding: 1.25rem 1.5rem;
  overflow: auto;
  height: 100%;
  max-width: 640px;
}
h1 {
  margin: 0 0 1rem;
  font-size: 1.25rem;
}
.block {
  margin-bottom: 1.5rem;
  padding-bottom: 1.25rem;
  border-bottom: 1px solid var(--border);
}
.block h2 {
  margin: 0 0 0.65rem;
  font-size: 1rem;
}
.block h3 {
  margin: 0.85rem 0 0.4rem;
  font-size: 0.9rem;
}
.name {
  margin: 0;
  font-weight: 600;
}
.email {
  margin: 0.2rem 0 0.75rem;
  color: var(--muted);
}
.check {
  display: flex;
  gap: 0.5rem;
  align-items: flex-start;
  line-height: 1.4;
}
label select {
  display: block;
  margin-top: 0.35rem;
  font: inherit;
  padding: 0.35rem 0.5rem;
}
.row {
  display: flex;
  gap: 0.5rem;
  margin-top: 0.65rem;
}
.folders ul {
  margin: 0;
  padding-left: 1.1rem;
  color: var(--muted);
  font-size: 0.85rem;
}
.err {
  color: var(--danger);
  font-size: 0.85rem;
}
</style>
