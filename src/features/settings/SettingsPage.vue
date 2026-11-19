<script setup lang="ts">
import { computed, ref } from 'vue';
import { useSettings } from './useSettings';
import { buildFolderTree, flattenFolderTree } from './folderTree';
import { formatTimestamp } from '../../shared/formatting/format';
import Button from '../../shared/ui/Button.vue';
import Avatar from '../../shared/ui/Avatar.vue';
import Dialog from '../../shared/ui/Dialog.vue';
import AddAccountDialog from '../accounts/AddAccountDialog.vue';
import type { Account, MailFolderSyncState } from '../../domain/entities';
import type { AccountId } from '../../domain/types';
import type { AppLocale } from '../../i18n';
import { asAppError } from '../../shared/errors/asAppError';

const {
  accountList,
  prefs,
  expandedId,
  savingId,
  syncingMailIds,
  syncingContactIds,
  refreshingFoldersId,
  togglingFolderKey,
  junkOverrideSavingKey,
  error,
  folderErrorById,
  mailProgressById,
  contactProgressById,
  toggleExpanded,
  mailStatusLabel,
  triggerMail,
  triggerContacts,
  refreshMailFolders,
  setFolderSyncEnabled,
  setFolderJunkOverride,
  folderToggleKey,
  updateAccountSettings,
  updateAccountAliases,
  signOutAccount,
  clearLocalData,
  setLocale,
} = useSettings();

function confirmClearLocalData(accountId: AccountId): void {
  const ok = window.confirm(
    '将清除此账号下的本地会话、邮件与联系人数据，账号与登录状态会保留。确定继续？',
  );
  if (!ok) return;
  void clearLocalData(accountId);
}

const addAccountOpen = ref(false);
/** 各账号别名输入框草稿 */
const aliasDraftById = ref<Record<string, string>>({});

function aliasDraft(accountId: AccountId): string {
  return aliasDraftById.value[accountId] ?? '';
}

function setAliasDraft(accountId: AccountId, value: string): void {
  aliasDraftById.value = { ...aliasDraftById.value, [accountId]: value };
}

function onAddAlias(account: Account): void {
  const raw = aliasDraft(account.id).trim();
  if (!raw) return;
  const next = [...account.aliases];
  const normalized = raw.toLowerCase();
  if (
    normalized === account.email.toLowerCase() ||
    next.some((a) => a.toLowerCase() === normalized)
  ) {
    setAliasDraft(account.id, '');
    return;
  }
  next.push(raw);
  setAliasDraft(account.id, '');
  void updateAccountAliases(account.id, next);
}

function onRemoveAlias(account: Account, alias: string): void {
  const next = account.aliases.filter((a) => a !== alias);
  void updateAccountAliases(account.id, next);
}

function onLocaleChange(e: Event) {
  setLocale((e.target as HTMLSelectElement).value as AppLocale);
}

function onIncludeInAll(account: Account, e: Event) {
  void updateAccountSettings(account.id, {
    include_in_all: (e.target as HTMLInputElement).checked,
  });
}

function folderStatusLabel(status: string, syncedAt: number | null): string {
  if (status === 'running') return '正在同步';
  if (status === 'error') return '部分内容可能未更新';
  if (syncedAt) return formatTimestamp(syncedAt);
  return '尚未完成首次同步';
}

function foldersFor(accountId: AccountId): MailFolderSyncState[] {
  return mailProgressById.value[accountId]?.folders ?? [];
}

const folderRowsByAccount = computed(() => {
  const out: Record<string, ReturnType<typeof flattenFolderTree>> = {};
  for (const a of accountList.value) {
    out[a.id] = flattenFolderTree(buildFolderTree(foldersFor(a.id)));
  }
  return out;
});

function onFolderSyncToggle(
  accountId: AccountId,
  folderId: string,
  e: Event,
): void {
  void setFolderSyncEnabled(
    accountId,
    folderId,
    (e.target as HTMLInputElement).checked,
  );
}

/** 文件夹设置：默认展示登录后发现的生效角色，只在用户修改时保存覆盖。 */
const junkDialog = ref<{
  accountId: AccountId;
  folderId: string;
  displayName: string;
  initialChoice: boolean;
  choice: boolean;
} | null>(null);
const junkDialogError = ref<string | null>(null);

const junkDialogBusy = computed(
  () =>
    junkDialog.value != null &&
    junkOverrideSavingKey.value ===
      folderToggleKey(junkDialog.value.accountId, junkDialog.value.folderId),
);

const currentJunkFolder = computed(() => {
  const dialog = junkDialog.value;
  if (!dialog) return undefined;
  return mailProgressById.value[dialog.accountId]?.folders.find(
    (folder) => folder.effective_role === 'junk',
  );
});

function openJunkDialog(
  accountId: AccountId,
  folder: MailFolderSyncState,
): void {
  junkDialogError.value = null;
  junkDialog.value = {
    accountId,
    folderId: folder.folder_id,
    displayName: folder.display_name || folder.folder_id,
    initialChoice: folder.effective_role === 'junk',
    choice: folder.effective_role === 'junk',
  };
}

function closeJunkDialog(): void {
  junkDialog.value = null;
  junkDialogError.value = null;
}

function setJunkChoice(choice: boolean): void {
  if (!junkDialog.value) return;
  junkDialog.value = { ...junkDialog.value, choice };
}

async function confirmJunkDialog(): Promise<void> {
  const dialog = junkDialog.value;
  if (!dialog || junkDialogBusy.value) return;
  if (dialog.choice === dialog.initialChoice) {
    closeJunkDialog();
    return;
  }
  try {
    await setFolderJunkOverride(
      dialog.accountId,
      dialog.folderId,
      dialog.choice,
    );
    closeJunkDialog();
  } catch (e) {
    junkDialogError.value = asAppError(e).message;
  }
}
</script>

<template>
  <section
    class="flex h-full min-h-0 w-full flex-col bg-surface-chat"
    aria-label="邮件同步管理"
  >
    <header
      class="shrink-0 border-b border-border-subtle bg-surface-header px-8 py-5"
    >
      <h1 class="m-0 text-xl font-semibold tracking-tight text-text-primary">
        邮件同步
      </h1>
      <p class="mt-1 mb-0 text-[13px] text-text-secondary">
        管理已登录邮箱的同步状态与显示偏好
      </p>
    </header>

    <div class="min-h-0 flex-1 overflow-auto">
      <div class="mx-auto flex w-full max-w-3xl flex-col gap-8 px-8 py-8">
        <!-- 全局配置 -->
        <section aria-labelledby="settings-global">
          <h2
            id="settings-global"
            class="m-0 mb-3 border-b border-border-subtle pb-2 text-[13px] font-semibold tracking-wide text-text-secondary uppercase"
          >
            全局配置
          </h2>
          <label class="flex max-w-xs flex-col gap-1.5 text-[14px] text-text-primary">
            界面语言
            <select
              class="w-full rounded-md border border-border-subtle bg-surface-input px-3 py-2 text-[14px] text-text-primary outline-none focus:border-brand"
              :value="prefs.preferences.locale"
              @change="onLocaleChange"
            >
              <option value="zh-CN">中文</option>
              <option value="en-US">English</option>
            </select>
          </label>
        </section>

        <!-- 邮件账号列表 -->
        <section aria-labelledby="settings-accounts">
          <div
            class="mb-3 flex flex-wrap items-end justify-between gap-3 border-b border-border-subtle pb-2"
          >
            <h2
              id="settings-accounts"
              class="m-0 text-[13px] font-semibold tracking-wide text-text-secondary uppercase"
            >
              邮件账号
            </h2>
            <Button variant="primary" @click="addAccountOpen = true">
              添加账号
            </Button>
          </div>

          <p v-if="error" class="mt-0 mb-3 text-[13px] text-danger" role="alert">
            {{ error }}
          </p>

          <p
            v-if="accountList.length === 0"
            class="m-0 rounded-md border border-border-subtle bg-surface-elevated px-4 py-6 text-[14px] text-text-secondary"
          >
            还没有已登录的邮箱。添加 Outlook 或 IMAP/SMTP 账号后，可在此查看同步状态。
          </p>

          <ul
            v-else
            class="m-0 list-none divide-y divide-border-subtle overflow-hidden rounded-lg border border-border-subtle bg-surface-elevated p-0"
            role="list"
          >
            <li
              v-for="account in accountList"
              :key="account.id"
              class="m-0"
            >
              <!-- 列表行：身份 + 常用同步 -->
              <div
                class="flex flex-wrap items-center gap-3 px-4 py-3.5"
              >
                <Avatar
                  :name="account.display_name || account.email"
                  :size="40"
                />
                <div class="min-w-0 flex-1">
                  <p
                    class="m-0 truncate text-[14px] font-medium text-text-primary"
                    v-no-i18n
                  >
                    {{ account.display_name || account.email }}
                  </p>
                  <p
                    class="m-0 truncate text-[12px] text-text-secondary"
                    v-no-i18n
                  >
                    {{ account.email }}
                  </p>
                  <p
                    class="mt-0.5 mb-0 truncate text-[12px] text-text-tertiary"
                  >
                    {{ mailStatusLabel(account.id) }}
                  </p>
                </div>
                <div class="flex flex-wrap items-center gap-2">
                  <Button
                    :disabled="syncingMailIds.has(account.id)"
                    @click="triggerMail(account.id)"
                  >
                    {{
                      syncingMailIds.has(account.id) ||
                      mailProgressById[account.id]?.mail_sync_status ===
                        'running'
                        ? '同步中…'
                        : '同步邮件'
                    }}
                  </Button>
                  <Button
                    :disabled="syncingContactIds.has(account.id)"
                    @click="triggerContacts(account.id)"
                  >
                    {{
                      syncingContactIds.has(account.id)
                        ? '请求中…'
                        : '同步联系人'
                    }}
                  </Button>
                  <Button
                    variant="ghost"
                    :aria-expanded="expandedId === account.id"
                    :aria-controls="`account-config-${account.id}`"
                    @click="toggleExpanded(account.id)"
                  >
                    {{ expandedId === account.id ? '收起' : '配置' }}
                  </Button>
                </div>
              </div>

              <!-- 展开：详细配置 -->
              <div
                v-if="expandedId === account.id"
                :id="`account-config-${account.id}`"
                class="border-t border-border-subtle bg-surface-muted px-4 py-4"
              >
                <div class="grid gap-4">
                  <label
                    class="flex cursor-pointer items-start gap-3 text-[14px] leading-snug text-text-primary"
                  >
                    <input
                      class="mt-0.5"
                      type="checkbox"
                      :checked="account.include_in_all"
                      :disabled="savingId === account.id"
                      @change="onIncludeInAll(account, $event)"
                    />
                    <span>
                      纳入「全部账号」列表
                      <span class="mt-1 block text-[12px] text-text-tertiary">
                        关闭后，会话/联系人/群组的「全部」视图将不再包含此账号
                      </span>
                    </span>
                  </label>

                  <div class="grid gap-2">
                    <div>
                      <p
                        class="m-0 text-[14px] font-medium text-text-primary"
                      >
                        别名
                      </p>
                      <p class="mt-0.5 mb-0 text-[12px] text-text-tertiary">
                        这些地址视为本账号（会话归属与发出判定）；不会用它们发信
                      </p>
                    </div>
                    <ul
                      v-if="account.aliases.length > 0"
                      class="m-0 flex list-none flex-col gap-1.5 p-0"
                    >
                      <li
                        v-for="alias in account.aliases"
                        :key="alias"
                        class="flex items-center gap-2 text-[13px] text-text-primary"
                      >
                        <span class="min-w-0 flex-1 truncate" v-no-i18n>{{
                          alias
                        }}</span>
                        <Button
                          variant="ghost"
                          :disabled="savingId === account.id"
                          @click="onRemoveAlias(account, alias)"
                        >
                          移除
                        </Button>
                      </li>
                    </ul>
                    <div class="flex flex-wrap items-center gap-2">
                      <input
                        class="min-w-[12rem] flex-1 rounded-md border border-border-subtle bg-surface-input px-3 py-2 text-[14px] text-text-primary outline-none focus:border-brand"
                        type="email"
                        placeholder="alias@example.com"
                        :value="aliasDraft(account.id)"
                        :disabled="savingId === account.id"
                        @input="
                          setAliasDraft(
                            account.id,
                            ($event.target as HTMLInputElement).value,
                          )
                        "
                        @keydown.enter.prevent="onAddAlias(account)"
                      />
                      <Button
                        :disabled="
                          savingId === account.id ||
                          !aliasDraft(account.id).trim()
                        "
                        @click="onAddAlias(account)"
                      >
                        添加
                      </Button>
                    </div>
                  </div>

                  <div class="grid gap-1 text-[13px] text-text-secondary">
                    <p
                      v-if="mailProgressById[account.id]?.mail_sync_error"
                      class="m-0 text-danger"
                    >
                      {{ mailProgressById[account.id]?.mail_sync_error }}
                    </p>
                    <p
                      v-if="contactProgressById[account.id]?.contact_synced_at"
                      class="m-0"
                    >
                      联系人最近同步于
                      {{
                        formatTimestamp(
                          contactProgressById[account.id]!.contact_synced_at,
                        )
                      }}
                    </p>
                    <p v-else class="m-0 text-text-tertiary">
                      联系人尚未同步（IMAP 账号通常无联系人增量）
                    </p>
                  </div>

                  <div>
                    <div
                      class="mb-2 flex flex-wrap items-center justify-between gap-2"
                    >
                      <h3
                        class="m-0 text-[12px] font-medium tracking-wide text-text-tertiary uppercase"
                      >
                        同步文件夹
                      </h3>
                      <Button
                        variant="ghost"
                        :disabled="refreshingFoldersId === account.id"
                        @click="refreshMailFolders(account.id)"
                      >
                        {{
                          refreshingFoldersId === account.id
                            ? '刷新中…'
                            : '刷新列表'
                        }}
                      </Button>
                    </div>
                    <p class="mt-0 mb-2 text-[12px] text-text-tertiary">
                      先拉取全部文件夹，勾选后才纳入「同步邮件」；默认不勾选。
                    </p>
                    <p
                      v-if="folderErrorById[account.id]"
                      class="mt-0 mb-2 text-[13px] text-danger"
                      role="alert"
                    >
                      {{ folderErrorById[account.id] }}
                    </p>
                    <p
                      v-else-if="refreshingFoldersId === account.id"
                      class="mt-0 mb-2 text-[13px] text-text-secondary"
                    >
                      正在从邮箱拉取文件夹树…
                    </p>
                    <p
                      v-if="
                        !(folderRowsByAccount[account.id]?.length) &&
                        refreshingFoldersId !== account.id
                      "
                      class="m-0 rounded-md border border-dashed border-border-subtle px-3 py-4 text-[13px] text-text-secondary"
                    >
                      尚未拉取文件夹列表，点击「刷新列表」从邮箱发现目录。
                    </p>
                    <ul
                      v-else
                      class="m-0 list-none divide-y divide-border-subtle overflow-hidden rounded-md border border-border-subtle bg-surface-elevated p-0"
                    >
                      <li
                        v-for="f in folderRowsByAccount[account.id]"
                        :key="f.folder_id"
                        class="flex flex-wrap items-center justify-between gap-2 px-3 py-2 text-[13px]"
                      >
                        <label
                          class="flex min-w-0 flex-1 cursor-pointer items-center gap-2 text-text-primary"
                          :style="{ paddingLeft: `${f.depth * 16}px` }"
                        >
                          <input
                            type="checkbox"
                            :checked="f.sync_enabled"
                            :disabled="
                              togglingFolderKey ===
                              folderToggleKey(account.id, f.folder_id)
                            "
                            @change="
                              onFolderSyncToggle(
                                account.id,
                                f.folder_id,
                                $event,
                              )
                            "
                          />
                          <span class="truncate font-medium" v-no-i18n>{{
                            f.display_name || f.folder_id
                          }}</span>
                        </label>
                        <span class="shrink-0 text-text-secondary">
                          {{
                            folderStatusLabel(f.sync_status, f.synced_at)
                          }}
                        </span>
                        <Button
                          variant="ghost"
                          class="!px-2 shrink-0"
                          :aria-label="`${f.display_name || f.folder_id} 的更多设置`"
                          @click="openJunkDialog(account.id, f)"
                        >
                          更多设置
                        </Button>
                      </li>
                    </ul>
                  </div>

                  <div class="flex flex-wrap gap-2 pt-1">
                    <Button
                      variant="secondary"
                      @click="confirmClearLocalData(account.id)"
                    >
                      清理数据
                    </Button>
                    <Button
                      variant="danger"
                      @click="signOutAccount(account.id)"
                    >
                      退出此账号
                    </Button>
                  </div>
                </div>
              </div>
            </li>
          </ul>
        </section>
      </div>
    </div>

    <Dialog
      :open="junkDialog != null"
      title="文件夹设置"
      confirm-label="保存"
      :confirming="junkDialogBusy"
      @close="closeJunkDialog"
      @confirm="confirmJunkDialog"
    >
      <p class="m-0 mb-3" v-no-i18n>{{ junkDialog?.displayName }}</p>
      <fieldset
        class="m-0 flex items-center gap-4 border-0 p-0 text-[14px]"
        :disabled="junkDialogBusy"
        aria-labelledby="folder-junk-role-label"
      >
        <span id="folder-junk-role-label" class="font-medium">是否垃圾文件夹：</span>
        <label class="flex cursor-pointer items-center gap-2">
          <input
            type="radio"
            name="folder-junk-role"
            class="accent-brand"
            :checked="junkDialog?.choice === true"
            @change="setJunkChoice(true)"
          />
          是
        </label>
        <label class="flex cursor-pointer items-center gap-2">
          <input
            type="radio"
            name="folder-junk-role"
            class="accent-brand"
            :checked="junkDialog?.choice === false"
            @change="setJunkChoice(false)"
          />
          否
        </label>
      </fieldset>
      <p class="mt-2 mb-0 text-[13px] text-text-secondary">
        当前垃圾文件夹：{{ currentJunkFolder?.display_name || currentJunkFolder?.folder_id || '未设置' }}
      </p>
      <p
        v-if="junkDialog?.choice && currentJunkFolder && currentJunkFolder.folder_id !== junkDialog.folderId"
        class="mt-2 mb-0 text-[13px] text-danger"
      >
        保存后将切换到「{{ junkDialog.displayName }}」，已有邮件不会移动。
      </p>
      <p v-if="junkDialogError" class="mt-2 mb-0 text-[13px] text-danger">
        {{ junkDialogError }}
      </p>
    </Dialog>

    <AddAccountDialog
      :open="addAccountOpen"
      @close="addAccountOpen = false"
    />
  </section>
</template>
