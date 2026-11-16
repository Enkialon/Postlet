import type { AccountPort } from '../../ports/accountPort';
import type { Account } from '../../domain/entities';
import type {
  UpdateAccountDisplaySettingsCommand,
  UpsertImapSmtpAccountCommand,
} from '../../domain/dto';
import type { AccountId } from '../../domain/types';
import type { MemoryStore } from './memoryStore';

export class MemoryAccountAdapter implements AccountPort {
  constructor(private readonly store: MemoryStore) {}

  async listAccounts(): Promise<Account[]> {
    return [...this.store.accounts.values()]
      .filter((a) => this.store.signedInIds.has(a.id))
      .sort((a, b) => a.created_at - b.created_at);
  }

  async getAccount(accountId: AccountId): Promise<Account> {
    return this.store.requireAccount(accountId);
  }

  async signIn(): Promise<Account> {
    const t = Date.now();
    const id = this.store.nextId('acc');
    const account: Account = {
      id,
      email: `user-${id.slice(-4)}@example.com`,
      display_name: `新用户 ${id.slice(-4)}`,
      aliases: [],
      tenant_id: null,
      home_account_id: null,
      contact_delta_link: null,
      mail_sync_triggered_at: null,
      contact_synced_at: null,
      mail_sync_status: 'idle',
      mail_sync_error: null,
      show_group_messages_in_contact_conversation: false,
      include_in_all: true,
      created_at: t,
      updated_at: t,
    };
    this.store.accounts.set(id, account);
    this.store.markSignedIn(id);
    this.store.folderStates.set(id, []);
    return account;
  }

  async upsertImapSmtpAccount(
    cmd: UpsertImapSmtpAccountCommand,
  ): Promise<Account> {
    const email = cmd.email.trim().toLowerCase();
    if (!email.includes('@')) {
      throw { code: 'validation', message: '邮箱地址无效', retryable: false };
    }
    if (cmd.auth_kind === 'password' && !cmd.password) {
      throw {
        code: 'validation',
        message: '密码认证须提供 password',
        retryable: false,
      };
    }
    const t = Date.now();
    const existing = [...this.store.accounts.values()].find(
      (a) => a.email.toLowerCase() === email,
    );
    if (existing) {
      existing.display_name = cmd.display_name.trim() || email;
      existing.updated_at = t;
      this.store.accounts.set(existing.id, existing);
      this.store.markSignedIn(existing.id);
      return { ...existing };
    }
    const id = this.store.nextId('acc');
    const account: Account = {
      id,
      email,
      display_name: cmd.display_name.trim() || email,
      aliases: [],
      tenant_id: null,
      home_account_id: null,
      contact_delta_link: null,
      mail_sync_triggered_at: null,
      contact_synced_at: null,
      mail_sync_status: 'idle',
      mail_sync_error: null,
      show_group_messages_in_contact_conversation: false,
      include_in_all: true,
      created_at: t,
      updated_at: t,
    };
    this.store.accounts.set(id, account);
    this.store.markSignedIn(id);
    this.store.folderStates.set(id, []);
    return account;
  }

  async signOut(accountId: AccountId): Promise<void> {
    this.store.requireAccount(accountId);
    // 只结束会话；账户行与邮件保留，再登录可复用
    this.store.markSignedOut(accountId);
  }

  async updateDisplaySettings(
    cmd: UpdateAccountDisplaySettingsCommand,
  ): Promise<Account> {
    const account = this.store.requireAccount(cmd.account_id);
    account.show_group_messages_in_contact_conversation =
      cmd.show_group_messages_in_contact_conversation;
    account.include_in_all = cmd.include_in_all;
    account.updated_at = Date.now();
    this.store.accounts.set(account.id, account);
    this.store.recomputeAllConversations(cmd.account_id);
    return { ...account };
  }

}
