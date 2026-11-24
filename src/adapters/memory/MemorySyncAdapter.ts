import type { SyncPort } from '../../ports/syncPort';
import type {
  ContactSyncProgress,
  MailSyncProgress,
  SetFolderJunkOverrideCommand,
  SetFolderSyncEnabledCommand,
  TriggerContactSyncCommand,
  TriggerMailSyncCommand,
} from '../../domain/dto';
import type { MailFolderSyncState, Message } from '../../domain/entities';
import type { AccountId } from '../../domain/types';
import { effectiveFolderRole, type MemoryStore } from './memoryStore';

export class MemorySyncAdapter implements SyncPort {
  private readonly timers = new Map<AccountId, ReturnType<typeof setTimeout>>();
  private syncGeneration = 0;

  constructor(private readonly store: MemoryStore) {}

  async triggerMailSync(cmd: TriggerMailSyncCommand): Promise<void> {
    const account = this.store.requireAccount(cmd.account_id);
    account.mail_sync_status = 'running';
    account.mail_sync_error = null;
    account.mail_sync_triggered_at = Date.now();
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
        current.mail_sync_error = null;
        current.updated_at = Date.now();
        this.store.accounts.set(current.id, current);
        const folders = this.store.folderStates.get(cmd.account_id) ?? [];
        const selected = cmd.folder_ids?.length
          ? folders.filter((f) => cmd.folder_ids!.includes(f.folder_id))
          : folders.filter((f) => f.sync_enabled);
        if (selected[0]) this.injectSyncedMail(cmd.account_id, selected[0]);
        for (const f of selected) {
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

  /** 假同步注入一封新邮件，方便验收列表刷新 */
  private injectSyncedMail(accountId: AccountId, folder: MailFolderSyncState): void {
    const account = this.store.accounts.get(accountId);
    if (!account) return;

    let target = this.store.findPrivateSession(accountId, 'alice@example.com');
    if (!target) {
      for (const s of this.store.sessions.values()) {
        if (
          s.account_id === accountId &&
          s.kind === 'private' &&
          s.merged_into_id == null
        ) {
          target = s;
          break;
        }
      }
    }
    if (!target || !target.normalized_address) return;

    this.syncGeneration += 1;
    const t = Date.now();
    const id = `msg-sync-${this.syncGeneration}`;
    const msg: Message = {
      id,
      account_id: accountId,
      session_id: target.id,
      remote_key: `g-${id}`,
      location_status: 'present',
      location_revision: 1,
      internet_message_id: null,
      in_reply_to: null,
      subject: `同步到来信 #${this.syncGeneration}`,
      body_preview: '这是 Memory 假同步注入的新邮件。',
      body_content_type: 'text',
      body_content: `假同步注入正文 #${this.syncGeneration}`,
      from_address: target.normalized_address,
      from_name: target.normalized_address.startsWith('alice')
        ? 'Alice Chen'
        : '',
      received_at: t,
      sent_at: t,
      is_read: false,
      is_draft: false,
      remote_is_read: null,
      remote_is_draft: null,
      remote_is_deleted: null,
      is_outbound: false,
      has_attachments: false,
      importance: 'normal',
      folder_id: folder.folder_id,
      parent_folder_name: folder.display_name,
      local_updated_at: t,
      remote_updated_at: t,
    };

    this.store.messages.set(this.store.messageKey(accountId, id), msg);
    this.store.recipients.set(this.store.messageKey(accountId, id), [
      {
        account_id: accountId,
        message_id: id,
        role: 'to',
        seq: 0,
        normalized_address: account.email,
        display_name: account.display_name,
      },
    ]);
    this.store.recomputeSession(accountId, target.id);
  }

  async triggerContactSync(cmd: TriggerContactSyncCommand): Promise<void> {
    const account = this.store.requireAccount(cmd.account_id);
    account.contact_synced_at = Date.now();
    account.updated_at = Date.now();
    this.store.accounts.set(account.id, account);

    // 注入一个新占位联系人，便于联系人页看到变化
    const n = String(Date.now()).slice(-4);
    const address = `synced${n}@example.com`;
    const existing = [...this.store.contacts.values()].some(
      (c) => c.account_id === cmd.account_id && c.normalized_address === address,
    );
    if (!existing) {
      const t = Date.now();
      const contact = {
        id: this.store.nextId('ct'),
        account_id: cmd.account_id,
        graph_id: `graph-synced-${n}`,
        display_name: `Synced ${n}`,
        normalized_address: address,
        source: 'graph' as const,
        created_at: t,
        updated_at: t,
      };
      this.store.contacts.set(
        this.store.contactKey(cmd.account_id, contact.id),
        contact,
      );
    }
  }

  async getMailSyncProgress(accountId: AccountId): Promise<MailSyncProgress> {
    const account = this.store.requireAccount(accountId);
    return {
      account_id: accountId,
      mail_sync_status: account.mail_sync_status,
      mail_sync_error: account.mail_sync_error,
      mail_sync_triggered_at: account.mail_sync_triggered_at,
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

  async refreshMailFolders(
    accountId: AccountId,
  ): Promise<MailFolderSyncState[]> {
    this.store.requireAccount(accountId);
    const t = Date.now();
    const existing = this.store.folderStates.get(accountId) ?? [];
    const byId = new Map(existing.map((f) => [f.folder_id, f]));
    const seed: Array<{
      id: string;
      parent: string | null;
      name: string;
      enabled: boolean;
    }> = [
      { id: 'inbox', parent: null, name: '收件箱', enabled: true },
      { id: 'sentitems', parent: null, name: '已发送', enabled: false },
      { id: 'drafts', parent: null, name: '草稿', enabled: false },
      {
        id: 'inbox/work',
        parent: 'inbox',
        name: '工作',
        enabled: false,
      },
    ];
    const next: MailFolderSyncState[] = seed.map((s) => {
      const prev = byId.get(s.id);
      const role = prev?.role ?? 'custom';
      const junk_override = prev?.junk_override ?? null;
      return {
        account_id: accountId,
        folder_id: s.id,
        protocol: 'graph',
        remote_binding: s.id,
        binding_revision: prev?.binding_revision ?? 1,
        is_retired: false,
        parent_folder_id: s.parent,
        display_name: s.name,
        sync_enabled: prev?.sync_enabled ?? s.enabled,
        generation: prev?.generation ?? null,
        sync_cursor: prev?.sync_cursor ?? null,
        page_cursor: prev?.page_cursor ?? null,
        synced_at: prev?.synced_at ?? null,
        role,
        role_source: prev?.role_source ?? 'unknown',
        junk_override,
        effective_role: effectiveFolderRole(role, junk_override),
        sync_status: prev?.sync_status ?? 'idle',
        last_error: prev?.last_error ?? null,
        updated_at: t,
      };
    });
    this.store.folderStates.set(accountId, next);
    return [...next];
  }

  async setFolderSyncEnabled(
    cmd: SetFolderSyncEnabledCommand,
  ): Promise<MailFolderSyncState> {
    this.store.requireAccount(cmd.account_id);
    const folders = this.store.folderStates.get(cmd.account_id) ?? [];
    const idx = folders.findIndex((f) => f.folder_id === cmd.folder_id);
    if (idx < 0) {
      throw new Error(`文件夹不存在: ${cmd.folder_id}`);
    }
    const updated: MailFolderSyncState = {
      ...folders[idx]!,
      sync_enabled: cmd.sync_enabled,
      updated_at: Date.now(),
    };
    folders[idx] = updated;
    this.store.folderStates.set(cmd.account_id, folders);
    return updated;
  }

  async setFolderWatchEnabled(accountId: AccountId, folderId: string, enabled: boolean): Promise<MailFolderSyncState> {
    const folders = this.store.folderStates.get(accountId) ?? [];
    const index = folders.findIndex((folder) => folder.folder_id === folderId && folder.protocol === 'imap');
    const folder = folders[index];
    if (!folder) throw new Error('只能设置有效的 IMAP 文件夹监听');
    const updated = { ...folder, watch_enabled: enabled };
    folders[index] = updated;
    this.store.folderStates.set(accountId, folders);
    return updated;
  }

  async setFolderJunkOverride(
    cmd: SetFolderJunkOverrideCommand,
  ): Promise<MailFolderSyncState> {
    this.store.requireAccount(cmd.account_id);
    const folders = this.store.folderStates.get(cmd.account_id) ?? [];
    const idx = folders.findIndex((f) => f.folder_id === cmd.folder_id);
    if (idx < 0) {
      throw new Error(`文件夹不存在: ${cmd.folder_id}`);
    }
    const prev = folders[idx]!;
    const updated: MailFolderSyncState = {
      ...prev,
      junk_override: cmd.junk_override,
      effective_role: effectiveFolderRole(prev.role, cmd.junk_override),
      updated_at: Date.now(),
    };
    this.store.folderStates.set(cmd.account_id, folders.map((folder, index) => {
      if (index === idx) return updated;
      if (updated.effective_role !== 'junk' || folder.effective_role !== 'junk') return folder;
      return { ...folder, junk_override: false, effective_role: effectiveFolderRole(folder.role, false) };
    }));
    return updated;
  }
}
