import type { MessagePort } from '../../ports/messagePort';
import type { MessageWithRecipients } from '../../domain/entities';
import type {
  DeleteMessagesCommand,
  MarkMessagesReadCommand,
  MoveMessageJunkCommand,
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
    if (!message) {
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
      if (!message) {
        this.store.failNotFound('邮件不存在');
      }
      // 乐观已读只改展示值；真实确认由后端 mutation 完成
      message.is_read = cmd.is_read;
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
      if (!message) continue;
      // 只记录本地删除意图，不伪造远端位置事实
      this.store.pendingDeletes.add(key);
      message.local_updated_at = Date.now();
      this.store.messages.set(key, message);
      touched.add(message.session_id);
    }
    for (const sessionId of touched) {
      this.store.recomputeSession(cmd.account_id, sessionId);
    }
  }

  async moveMessageJunk(
    cmd: MoveMessageJunkCommand,
  ): Promise<MessageWithRecipients> {
    this.store.requireAccount(cmd.account_id);
    const key = this.store.messageKey(cmd.account_id, cmd.message_id);
    const message = this.store.messages.get(key);
    if (!message) {
      this.store.failNotFound('邮件不存在');
    }
    if (message.location_status !== 'present' || !message.remote_key) {
      this.store.failValidation('邮件没有有效的远端位置，无法移动');
    }
    if (this.store.pendingDeletes.has(key)) {
      this.store.failConflict('邮件已在删除流程中，无法移动');
    }
    // 目标选择与后端一致：不按名称猜，歧义时不移动
    const folders = this.store.folderStates.get(cmd.account_id) ?? [];
    const candidates = folders.filter((f) =>
      cmd.direction === 'to_junk'
        ? f.effective_role === 'junk'
        : f.effective_role === 'inbox',
    );
    const standard = candidates.filter((f) => f.role_source === 'server');
    const chosen =
      standard.length === 1
        ? standard[0]
        : candidates.length === 1
          ? candidates[0]
          : undefined;
    if (!chosen || chosen.folder_id === message.folder_id) {
      this.store.failValidation(
        cmd.direction === 'to_junk'
          ? '未找到唯一的垃圾邮件文件夹，请先在设置中确认文件夹角色'
          : '未找到唯一的收件箱文件夹，请先在设置中确认文件夹角色',
      );
    }
    // 只记录意图；真实位置由后续同步确认
    this.store.pendingJunkMoves.set(key, cmd.direction);
    return this.store.withRecipients(cmd.account_id, cmd.message_id);
  }
}
