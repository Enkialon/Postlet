import { inject, ref, watch } from 'vue';
import type { ContactSyncProgress, MailSyncProgress } from '../../domain/dto';
import { portsKey, navigationKey } from '../../app/provide';
import { asAppError } from '../../shared/errors/asAppError';
import { formatTimestamp } from '../../shared/formatting/format';

const mailProgress = ref<MailSyncProgress | null>(null);
const contactProgress = ref<ContactSyncProgress | null>(null);
const contactTriggerHint = ref<string | null>(null);
const syncError = ref<string | null>(null);

let mailTimer: ReturnType<typeof setTimeout> | null = null;
let listTimer: ReturnType<typeof setTimeout> | null = null;
let backoffMs = 2000;
let focused = true;
let bound = false;
let onForegroundRefresh: (() => void) | undefined;

function clearTimers(): void {
  if (mailTimer) clearTimeout(mailTimer);
  if (listTimer) clearTimeout(listTimer);
  mailTimer = null;
  listTimer = null;
}

export function useSyncStatus(options?: {
  onForegroundRefresh?: () => void;
}) {
  const ports = inject(portsKey)!;
  const nav = inject(navigationKey)!;

  if (options?.onForegroundRefresh) {
    onForegroundRefresh = options.onForegroundRefresh;
  }

  async function refreshProgress(): Promise<void> {
    const accountId = nav.accountId.value;
    if (!accountId) return;
    const session = nav.sessionVersion.value;
    const wasRunning = mailProgress.value?.mail_sync_status === 'running';
    try {
      const mail = await ports.sync.getMailSyncProgress(accountId);
      const contact = await ports.sync.getContactSyncProgress(accountId);
      if (session !== nav.sessionVersion.value) return;
      mailProgress.value = mail;
      contactProgress.value = contact;
      syncError.value = null;
      backoffMs = 2000;
      if (wasRunning && mail.mail_sync_status !== 'running') {
        onForegroundRefresh?.();
      }
    } catch (e) {
      syncError.value = asAppError(e).message;
      backoffMs = Math.min(backoffMs * 2, 60_000);
    }
  }

  function scheduleMailPoll(): void {
    if (mailTimer) clearTimeout(mailTimer);
    const delay =
      mailProgress.value?.mail_sync_status === 'running' ? 2000 : backoffMs;
    mailTimer = setTimeout(async () => {
      if (!focused) return;
      await refreshProgress();
      scheduleMailPoll();
    }, delay);
  }

  function scheduleListRefresh(): void {
    if (listTimer) clearTimeout(listTimer);
    listTimer = setTimeout(() => {
      if (!focused) return;
      onForegroundRefresh?.();
      scheduleListRefresh();
    }, 15_000);
  }

  async function triggerMail(): Promise<void> {
    const accountId = nav.accountId.value;
    if (!accountId) return;
    await ports.sync.triggerMailSync({
      account_id: accountId,
      folder_ids: null,
    });
    await refreshProgress();
    scheduleMailPoll();
  }

  async function triggerContacts(): Promise<void> {
    const accountId = nav.accountId.value;
    if (!accountId) return;
    await ports.sync.triggerContactSync({ account_id: accountId });
    contactTriggerHint.value = '已请求同步联系人';
    await refreshProgress();
  }

  function mailStatusLabel(): string {
    const p = mailProgress.value;
    if (!p) return '';
    if (p.mail_sync_status === 'running') return '正在同步';
    if (p.mail_sync_status === 'error') return '部分内容可能未更新';
    if (p.mail_sync_status === 'idle' && !p.mail_synced_at) {
      return '尚未完成首次同步';
    }
    if (p.mail_synced_at) {
      return `上次邮件同步于 ${formatTimestamp(p.mail_synced_at)}`;
    }
    return '';
  }

  function onFocus(): void {
    focused = true;
    void refreshProgress().then(() => {
      scheduleMailPoll();
      scheduleListRefresh();
      onForegroundRefresh?.();
    });
  }

  function onBlur(): void {
    focused = false;
    clearTimers();
  }

  if (!bound) {
    bound = true;
    watch(
      () => [nav.accountId.value, nav.sessionVersion.value],
      () => {
        clearTimers();
        mailProgress.value = null;
        contactProgress.value = null;
        contactTriggerHint.value = null;
        if (!nav.accountId.value) return;
        void refreshProgress().then(() => {
          scheduleMailPoll();
          if (focused) scheduleListRefresh();
        });
      },
      { immediate: true },
    );

    if (typeof window !== 'undefined') {
      window.addEventListener('focus', onFocus);
      window.addEventListener('blur', onBlur);
    }
  }

  return {
    mailProgress,
    contactProgress,
    contactTriggerHint,
    syncError,
    triggerMail,
    triggerContacts,
    refreshProgress,
    mailStatusLabel,
  };
}
