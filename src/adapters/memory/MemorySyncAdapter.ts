import type { SyncPort } from '../../ports/syncPort';
import type {
  ContactSyncProgress,
  MailSyncProgress,
  TriggerContactSyncCommand,
  TriggerMailSyncCommand,
} from '../../domain/dto';
import type { AccountId } from '../../domain/types';
import type { MemoryStore } from './memoryStore';

export class MemorySyncAdapter implements SyncPort {
  private readonly timers = new Map<AccountId, ReturnType<typeof setTimeout>>();

  constructor(private readonly store: MemoryStore) {}

  async triggerMailSync(cmd: TriggerMailSyncCommand): Promise<void> {
    const account = this.store.requireAccount(cmd.account_id);
    account.mail_sync_status = 'running';
    account.mail_sync_error = null;
    account.updated_at = Date.now();
    this.store.accounts.set(account.id, account);

    const existing = this.timers.get(cmd.account_id);
    if (existing) clearTimeout(existing);

    this.timers.set(
      cmd.account_id,
      setTimeout(() => {
        const current = this.store.accounts.get(cmd.account_id);
        if (!current) return;
        current.mail_sync_status = 'idle';
        current.mail_synced_at = Date.now();
        current.mail_sync_error = null;
        current.updated_at = Date.now();
        this.store.accounts.set(current.id, current);
        const folders = this.store.folderStates.get(cmd.account_id) ?? [];
        for (const f of folders) {
          f.sync_status = 'idle';
          f.synced_at = Date.now();
          f.last_error = null;
          f.updated_at = Date.now();
        }
        this.store.folderStates.set(cmd.account_id, folders);
        this.timers.delete(cmd.account_id);
      }, 1500),
    );
  }

  async triggerContactSync(cmd: TriggerContactSyncCommand): Promise<void> {
    const account = this.store.requireAccount(cmd.account_id);
    account.contact_synced_at = Date.now();
    account.updated_at = Date.now();
    this.store.accounts.set(account.id, account);
  }

  async getMailSyncProgress(accountId: AccountId): Promise<MailSyncProgress> {
    const account = this.store.requireAccount(accountId);
    return {
      account_id: accountId,
      mail_sync_status: account.mail_sync_status,
      mail_sync_error: account.mail_sync_error,
      mail_synced_at: account.mail_synced_at,
      folders: [...(this.store.folderStates.get(accountId) ?? [])],
    };
  }

  async getContactSyncProgress(
    accountId: AccountId,
  ): Promise<ContactSyncProgress> {
    const account = this.store.requireAccount(accountId);
    return {
      account_id: accountId,
      contact_synced_at: account.contact_synced_at,
      contact_delta_link: account.contact_delta_link,
    };
  }
}
