import type { AccountPort } from '../../ports/accountPort';
import type { Account } from '../../domain/entities';
import type { UpdateAccountDisplaySettingsCommand } from '../../domain/dto';
import type { AccountId } from '../../domain/types';
import type { MemoryStore } from './memoryStore';

export class MemoryAccountAdapter implements AccountPort {
  constructor(private readonly store: MemoryStore) {}

  async listAccounts(): Promise<Account[]> {
    return [...this.store.accounts.values()].sort(
      (a, b) => a.created_at - b.created_at,
    );
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
      tenant_id: 'tenant-demo',
      home_account_id: `home-${id}`,
      contact_delta_link: null,
      mail_synced_at: null,
      contact_synced_at: null,
      mail_sync_status: 'idle',
      mail_sync_error: null,
      show_group_messages_in_contact_conversation: false,
      created_at: t,
      updated_at: t,
    };
    this.store.accounts.set(id, account);
    this.store.folderStates.set(id, []);
    return account;
  }

  async signOut(accountId: AccountId): Promise<void> {
    this.store.requireAccount(accountId);
    this.store.accounts.delete(accountId);
    for (const key of [...this.store.contacts.keys()]) {
      if (key.startsWith(`${accountId}:`)) this.store.contacts.delete(key);
    }
    for (const key of [...this.store.conversations.keys()]) {
      if (key.startsWith(`${accountId}:`)) {
        this.store.conversations.delete(key);
        this.store.conversationMessages.delete(key);
      }
    }
    for (const key of [...this.store.messages.keys()]) {
      if (key.startsWith(`${accountId}:`)) {
        this.store.messages.delete(key);
        this.store.recipients.delete(key);
        this.store.pendingBodies.delete(key);
      }
    }
    for (const key of [...this.store.groups.keys()]) {
      if (key.startsWith(`${accountId}:`)) {
        this.store.groups.delete(key);
        this.store.participants.delete(key);
      }
    }
    this.store.folderStates.delete(accountId);
  }

  async updateDisplaySettings(
    cmd: UpdateAccountDisplaySettingsCommand,
  ): Promise<Account> {
    const account = this.store.requireAccount(cmd.account_id);
    account.show_group_messages_in_contact_conversation =
      cmd.show_group_messages_in_contact_conversation;
    account.updated_at = Date.now();
    this.store.accounts.set(account.id, account);
    this.store.recomputeAllConversations(cmd.account_id);
    return { ...account };
  }

  async loadDemoWorkspace(): Promise<Account> {
    this.store.clearAll();
    this.store.seedDemo();
    return { ...this.store.requireAccount('acc-demo') };
  }
}
