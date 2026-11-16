import type { MessagePort } from '../../ports/messagePort';
import type { MessageWithRecipients } from '../../domain/entities';
import type {
  DeleteMessagesCommand,
  MarkMessagesReadCommand,
} from '../../domain/dto';
import type { AccountId, MessageId } from '../../domain/types';
import type { MemoryStore } from './memoryStore';

export class MemoryMessageAdapter implements MessagePort {
  private readonly bodyInflight = new Set<string>();

  constructor(private readonly store: MemoryStore) {}

  async getMessage(
    accountId: AccountId,
    messageId: MessageId,
  ): Promise<MessageWithRecipients> {
    this.store.requireAccount(accountId);
    return this.store.withRecipients(accountId, messageId);
  }

  async ensureMessageBody(
    accountId: AccountId,
    messageId: MessageId,
  ): Promise<MessageWithRecipients> {
    this.store.requireAccount(accountId);
    const key = this.store.messageKey(accountId, messageId);
    if (this.bodyInflight.has(key)) {
      while (this.bodyInflight.has(key)) {
        await new Promise((r) => setTimeout(r, 20));
      }
      return this.store.withRecipients(accountId, messageId);
    }

    const message = this.store.messages.get(key);
    if (!message || message.sync_status === 'deleted_remote') {
      this.store.failNotFound('邮件不存在');
    }
    if (message.body_content !== null) {
      return this.store.withRecipients(accountId, messageId);
    }

    this.bodyInflight.add(key);
    try {
      await new Promise((r) => setTimeout(r, 80));
      const pending = this.store.pendingBodies.get(key);
      message.body_content = pending ?? '';
      message.local_updated_at = Date.now();
      this.store.messages.set(key, message);
      this.store.pendingBodies.delete(key);
      return this.store.withRecipients(accountId, messageId);
    } finally {
      this.bodyInflight.delete(key);
    }
  }

  async markRead(cmd: MarkMessagesReadCommand): Promise<void> {
    this.store.requireAccount(cmd.account_id);
    const touched = new Set<string>();

    for (const messageId of cmd.message_ids) {
      const key = this.store.messageKey(cmd.account_id, messageId);
      const message = this.store.messages.get(key);
      if (!message || message.sync_status === 'deleted_remote') {
        this.store.failNotFound('邮件不存在');
      }
      message.is_read = cmd.is_read;
      message.sync_status = 'pending_local';
      message.local_updated_at = Date.now();
      this.store.messages.set(key, message);
      touched.add(message.session_id);
    }

    for (const sessionId of touched) {
      this.store.recomputeSession(cmd.account_id, sessionId);
    }
  }

  async deleteMessages(cmd: DeleteMessagesCommand): Promise<void> {
    this.store.requireAccount(cmd.account_id);
    const touched = new Set<string>();
    for (const messageId of cmd.message_ids) {
      const key = this.store.messageKey(cmd.account_id, messageId);
      const message = this.store.messages.get(key);
      if (!message || message.sync_status === 'deleted_remote') continue;
      message.sync_status = 'deleted_remote';
      message.local_updated_at = Date.now();
      this.store.messages.set(key, message);
      touched.add(message.session_id);
    }
    for (const sessionId of touched) {
      this.store.recomputeSession(cmd.account_id, sessionId);
    }
  }
}
