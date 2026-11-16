import { computed, inject, onUnmounted, ref, watch } from 'vue';
import type { Account, MailFolderSyncState } from '../../domain/entities';
import type {
  ContactSyncProgress,
  MailSyncProgress,
} from '../../domain/dto';
import type { AccountId, FolderId } from '../../domain/types';
import { portsKey } from '../../app/provide';
import { useAccounts } from '../accounts/useAccounts';
import { useConversations } from '../conversations/useConversations';
import { useMessages } from '../conversations/useMessages';
import { usePreferences } from '../../app/preferences';
import type { AppLocale } from '../../i18n';
import { asAppError } from '../../shared/errors/asAppError';
import { formatTimestamp } from '../../shared/formatting/format';

const POLL_IDLE_MS = 8_000;
const POLL_RUNNING_MS = 2_000;

export function useSettings() {
  const ports = inject(portsKey)!;
  const accountsApi = useAccounts();
  const conversations = useConversations();
  const messages = useMessages();
  const prefs = usePreferences();

  const expandedId = ref<AccountId | null>(null);
  const savingId = ref<AccountId | null>(null);
  const syncingMailIds = ref<Set<AccountId>>(new Set());
  const syncingContactIds = ref<Set<AccountId>>(new Set());
  const refreshingFoldersId = ref<AccountId | null>(null);
  const togglingFolderKey = ref<string | null>(null);
  const error = ref<string | null>(null);
  const folderErrorById = ref<Record<string, string>>({});
  const mailProgressById = ref<Record<string, MailSyncProgress>>({});
  const contactProgressById = ref<Record<string, ContactSyncProgress>>({});

  let pollTimer: ReturnType<typeof setTimeout> | null = null;
  /** 作废在途 getMailSyncProgress，避免盖掉刚刷新的文件夹列表 */
  let progressEpoch = 0;

  const accountList = computed(() => accountsApi.accounts.value);

  function folderToggleKey(accountId: AccountId, folderId: FolderId): string {
    return `${accountId}:${folderId}`;
  }

  function patchFolders(
    accountId: AccountId,
    folders: MailFolderSyncState[],
  ): void {
    const prev = mailProgressById.value[accountId];
    if (!prev) {
      mailProgressById.value = {
        ...mailProgressById.value,
        [accountId]: {
          account_id: accountId,
          mail_sync_status: 'idle',
          mail_sync_error: null,
          mail_sync_triggered_at: null,
          folders,
        },
      };
      return;
    }
    mailProgressById.value = {
      ...mailProgressById.value,
      [accountId]: { ...prev, folders },
    };
  }

  async function toggleExpanded(accountId: AccountId): Promise<void> {
    if (expandedId.value === accountId) {
      expandedId.value = null;
      return;
    }
    expandedId.value = accountId;
    await refreshProgressFor(accountId);
    const folders = mailProgressById.value[accountId]?.folders ?? [];
    if (folders.length === 0) {
      await refreshMailFolders(accountId);
    }
  }

  function patchAccount(updated: Account): void {
    accountsApi.accounts.value = accountsApi.accounts.value.map((a) =>
      a.id === updated.id ? updated : a,
    );
    if (accountsApi.current.value?.id === updated.id) {
      accountsApi.replaceCurrent(updated);
    }
  }

  async function refreshProgressFor(accountId: AccountId): Promise<void> {
    const epoch = progressEpoch;
    try {
      const [mail, contact] = await Promise.all([
        ports.sync.getMailSyncProgress(accountId),
        ports.sync.getContactSyncProgress(accountId),
      ]);
      if (epoch !== progressEpoch) return;
      // 正在发现目录时不要用旧进度覆盖文件夹树
      if (refreshingFoldersId.value === accountId) {
        const prevFolders = mailProgressById.value[accountId]?.folders ?? [];
        mailProgressById.value = {
          ...mailProgressById.value,
          [accountId]: {
            ...mail,
            folders: prevFolders.length > 0 ? prevFolders : mail.folders,
          },
        };
      } else {
        mailProgressById.value = {
          ...mailProgressById.value,
          [accountId]: mail,
        };
      }
      contactProgressById.value = {
        ...contactProgressById.value,
        [accountId]: contact,
      };
    } catch (e) {
      if (epoch !== progressEpoch) return;
      error.value = asAppError(e).message;
    }
  }

  async function refreshAllProgress(): Promise<void> {
    const ids = accountList.value.map((a) => a.id);
    await Promise.all(ids.map((id) => refreshProgressFor(id)));
  }

  function anyMailRunning(): boolean {
    return Object.values(mailProgressById.value).some(
      (p) => p.mail_sync_status === 'running',
    );
  }

  function schedulePoll(): void {
    if (pollTimer) clearTimeout(pollTimer);
    const delay = anyMailRunning() ? POLL_RUNNING_MS : POLL_IDLE_MS;
    pollTimer = setTimeout(async () => {
      if (accountList.value.length === 0) {
        schedulePoll();
        return;
      }
      await refreshAllProgress();
      schedulePoll();
    }, delay);
  }

  function mailStatusLabel(accountId: AccountId): string {
    const p = mailProgressById.value[accountId];
    const account = accountList.value.find((a) => a.id === accountId);
    const status = p?.mail_sync_status ?? account?.mail_sync_status;
    const triggeredAt =
      p?.mail_sync_triggered_at ?? account?.mail_sync_triggered_at ?? null;
    const syncError = p?.mail_sync_error ?? account?.mail_sync_error ?? null;
    const folderOk = (p?.folders ?? []).some((f) => f.synced_at != null);
    if (status === 'running') return '正在同步';
    if (status === 'error') {
      return syncError ? `同步异常：${syncError}` : '部分内容可能未更新';
    }
    if (status === 'idle' && !folderOk) return '尚未完成首次同步';
    if (triggeredAt) return `上次发起 ${formatTimestamp(triggeredAt)}`;
    return '未知状态';
  }

  function isMailRunning(accountId: AccountId): boolean {
    return mailProgressById.value[accountId]?.mail_sync_status === 'running';
  }

  async function triggerMail(accountId: AccountId): Promise<void> {
    if (syncingMailIds.value.has(accountId) || isMailRunning(accountId)) return;
    error.value = null;
    const next = new Set(syncingMailIds.value);
    next.add(accountId);
    syncingMailIds.value = next;
    try {
      // RS 侧立即返回，同步在后台跑；状态靠进度轮询
      await ports.sync.triggerMailSync({
        account_id: accountId,
        folder_ids: null,
      });
      // 短等 Running 落库，避免按钮闪一下「同步邮件」
      for (let i = 0; i < 8; i += 1) {
        await refreshProgressFor(accountId);
        if (isMailRunning(accountId)) break;
        await new Promise((r) => setTimeout(r, 50));
      }
      schedulePoll();
    } catch (e) {
      error.value = asAppError(e).message;
    } finally {
      const done = new Set(syncingMailIds.value);
      done.delete(accountId);
      syncingMailIds.value = done;
    }
  }

  async function triggerContacts(accountId: AccountId): Promise<void> {
    if (syncingContactIds.value.has(accountId)) return;
    error.value = null;
    const next = new Set(syncingContactIds.value);
    next.add(accountId);
    syncingContactIds.value = next;
    try {
      await ports.sync.triggerContactSync({ account_id: accountId });
      await refreshProgressFor(accountId);
    } catch (e) {
      error.value = asAppError(e).message;
    } finally {
      const done = new Set(syncingContactIds.value);
      done.delete(accountId);
      syncingContactIds.value = done;
    }
  }

  async function updateAccountSettings(
    accountId: AccountId,
    patch: {
      show_group_messages_in_contact_conversation?: boolean;
      include_in_all?: boolean;
    },
  ): Promise<void> {
    const account = accountList.value.find((a) => a.id === accountId);
    if (!account || savingId.value) return;

    const next = {
      show_group_messages_in_contact_conversation:
        patch.show_group_messages_in_contact_conversation ??
        account.show_group_messages_in_contact_conversation,
      include_in_all: patch.include_in_all ?? account.include_in_all,
    };
    const optimistic: Account = { ...account, ...next };
    patchAccount(optimistic);
    savingId.value = accountId;
    error.value = null;
    try {
      const updated = await ports.account.updateDisplaySettings({
        account_id: accountId,
        ...next,
      });
      patchAccount(updated);
      conversations.invalidate();
      if (accountsApi.current.value?.id === accountId) {
        await messages.loadInitial();
      }
    } catch (e) {
      patchAccount(account);
      error.value = asAppError(e).message;
    } finally {
      savingId.value = null;
    }
  }

  async function refreshMailFolders(accountId: AccountId): Promise<void> {
    if (refreshingFoldersId.value === accountId) return;
    error.value = null;
    folderErrorById.value = { ...folderErrorById.value, [accountId]: '' };
    refreshingFoldersId.value = accountId;
    try {
      const folders = await ports.sync.refreshMailFolders(accountId);
      progressEpoch += 1;
      patchFolders(accountId, folders);
      if (folders.length === 0) {
        folderErrorById.value = {
          ...folderErrorById.value,
          [accountId]: '未发现任何文件夹，请稍后重试',
        };
      }
    } catch (e) {
      const message = asAppError(e).message;
      error.value = message;
      folderErrorById.value = {
        ...folderErrorById.value,
        [accountId]: message,
      };
    } finally {
      refreshingFoldersId.value = null;
    }
  }

  async function setFolderSyncEnabled(
    accountId: AccountId,
    folderId: FolderId,
    syncEnabled: boolean,
  ): Promise<void> {
    const key = folderToggleKey(accountId, folderId);
    if (togglingFolderKey.value === key) return;
    error.value = null;
    const prev = mailProgressById.value[accountId]?.folders ?? [];
    const optimistic = prev.map((f) =>
      f.folder_id === folderId
        ? { ...f, sync_enabled: syncEnabled, updated_at: Date.now() }
        : f,
    );
    patchFolders(accountId, optimistic);
    togglingFolderKey.value = key;
    try {
      const updated = await ports.sync.setFolderSyncEnabled({
        account_id: accountId,
        folder_id: folderId,
        sync_enabled: syncEnabled,
      });
      const next = (mailProgressById.value[accountId]?.folders ?? []).map(
        (f) => (f.folder_id === updated.folder_id ? updated : f),
      );
      patchFolders(accountId, next);
    } catch (e) {
      patchFolders(accountId, prev);
      error.value = asAppError(e).message;
    } finally {
      togglingFolderKey.value = null;
    }
  }

  async function signOutAccount(accountId: AccountId): Promise<void> {
    error.value = null;
    try {
      await accountsApi.signOutAccount(accountId);
      if (expandedId.value === accountId) expandedId.value = null;
      const { [accountId]: _m, ...mailRest } = mailProgressById.value;
      const { [accountId]: _c, ...contactRest } = contactProgressById.value;
      mailProgressById.value = mailRest;
      contactProgressById.value = contactRest;
      void _m;
      void _c;
    } catch (e) {
      error.value = asAppError(e).message;
    }
  }

  function setLocale(locale: AppLocale): void {
    prefs.setLocale(locale);
  }

  watch(
    accountList,
    (list) => {
      void refreshAllProgress().then(() => schedulePoll());
      if (
        expandedId.value &&
        !list.some((a) => a.id === expandedId.value)
      ) {
        expandedId.value = null;
      }
    },
    { immediate: true },
  );

  onUnmounted(() => {
    if (pollTimer) clearTimeout(pollTimer);
    pollTimer = null;
  });

  return {
    accountsApi,
    accountList,
    prefs,
    expandedId,
    savingId,
    syncingMailIds,
    syncingContactIds,
    refreshingFoldersId,
    togglingFolderKey,
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
    folderToggleKey,
    updateAccountSettings,
    signOutAccount,
    setLocale,
    refreshAllProgress,
  };
}
