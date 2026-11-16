import type { AccountPort } from '../../ports/accountPort';
import type { Account } from '../../domain/entities';
import type {
  UpdateAccountAliasesCommand,
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
    this.store.markSignedOut(accountId);
  }

  async updateDisplaySettings(
    cmd: UpdateAccountDisplaySettingsCommand,
  ): Promise<Account> {
    const account = this.store.requireAccount(cmd.account_id);
    account.include_in_all = cmd.include_in_all;
    account.updated_at = Date.now();
    this.store.accounts.set(account.id, account);
    return { ...account };
  }

  async updateAliases(cmd: UpdateAccountAliasesCommand): Promise<Account> {
    const account = this.store.requireAccount(cmd.account_id);
    const primary = account.email.toLowerCase();
    const aliases: string[] = [];
    for (const raw of cmd.aliases) {
      const trimmed = raw.trim().toLowerCase();
      if (!trimmed) continue;
      if (!trimmed.includes('@')) {
        throw {
          code: 'validation',
          message: `邮箱地址无效: ${trimmed}`,
          retryable: false,
        };
      }
      if (trimmed === primary) {
        throw {
          code: 'validation',
          message: '别名不能与主邮箱相同',
          retryable: false,
        };
      }
      if (aliases.includes(trimmed)) continue;
      for (const other of this.store.accounts.values()) {
        if (other.id === account.id) continue;
        if (
          other.email.toLowerCase() === trimmed ||
          other.aliases.some((a) => a.toLowerCase() === trimmed)
        ) {
          throw {
            code: 'validation',
            message: `别名已被其他账号使用: ${trimmed}`,
            retryable: false,
          };
        }
      }
      aliases.push(trimmed);
    }
    account.aliases = aliases;
    account.updated_at = Date.now();
    this.store.accounts.set(account.id, account);
    return { ...account };
  }
}
