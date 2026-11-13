import type { ComposePort } from '../../ports/composePort';
import type {
  ComposeDraft,
  ComposeRecipientRef,
  SaveComposeDraftCommand,
  SendComposedMailCommand,
} from '../../domain/dto';
import type {
  Message,
  MessageRecipient,
  MessageWithRecipients,
} from '../../domain/entities';
import type {
  AccountId,
  ConversationId,
  MessageId,
} from '../../domain/types';
import type { MemoryStore } from './memoryStore';

function stripHtml(html: string): string {
  return html
    .replace(/<br\s*\/?>/gi, '\n')
    .replace(/<\/p>/gi, '\n')
    .replace(/<[^>]+>/g, '')
    .replace(/&nbsp;/g, ' ')
    .replace(/&amp;/g, '&')
    .replace(/&lt;/g, '<')
    .replace(/&gt;/g, '>')
    .replace(/\s+/g, ' ')
    .trim();
}

function replySubject(original: string): string {
  const t = original.trim();
  if (!t) return '';
  if (/^re:\s+/i.test(t)) return t;
  return `Re: ${t}`;
}

function normalizeRecipients(
  list: ComposeRecipientRef[],
  store: MemoryStore,
): ComposeRecipientRef[] {
  const seen = new Set<string>();
  const out: ComposeRecipientRef[] = [];
  for (const raw of list) {
    const addr = store.normalizeEmail(raw.address);
    if (!addr || seen.has(addr)) continue;
    seen.add(addr);
    out.push({
      address: addr,
      display_name: raw.display_name.trim() || addr,
    });
  }
  return out;
}

export class MemoryComposeAdapter implements ComposePort {
  private readonly drafts = new Map<string, ComposeDraft>();

  constructor(private readonly store: MemoryStore) {}

  private draftKey(accountId: AccountId, conversationId: ConversationId): string {
    return `${accountId}:${conversationId}`;
  }

  async getDraft(
    accountId: AccountId,
    conversationId: ConversationId,
  ): Promise<ComposeDraft | null> {
    this.store.requireAccount(accountId);
    return this.drafts.get(this.draftKey(accountId, conversationId)) ?? null;
  }

  async saveDraft(cmd: SaveComposeDraftCommand): Promise<ComposeDraft> {
    this.store.requireAccount(cmd.account_id);
    const key = this.draftKey(cmd.account_id, cmd.conversation_id);
    const draft: ComposeDraft = {
      account_id: cmd.account_id,
      conversation_id: cmd.conversation_id,
      reply_to_message_id: cmd.reply_to_message_id,
      to: normalizeRecipients(cmd.to, this.store),
      cc: normalizeRecipients(cmd.cc, this.store),
      bcc: normalizeRecipients(cmd.bcc, this.store),
      subject: cmd.subject,
      body_html: cmd.body_html,
      attachments: cmd.attachments.map((a) => ({ ...a })),
      updated_at: Date.now(),
    };
    this.drafts.set(key, draft);
    return draft;
  }

  async clearDraft(
    accountId: AccountId,
    conversationId: ConversationId,
  ): Promise<void> {
    this.store.requireAccount(accountId);
    this.drafts.delete(this.draftKey(accountId, conversationId));
  }

  async send(cmd: SendComposedMailCommand): Promise<MessageWithRecipients> {
    const account = this.store.requireAccount(cmd.account_id);
    const convKey = this.store.conversationKey(
      cmd.account_id,
      cmd.conversation_id,
    );
    const conversation = this.store.conversations.get(convKey);
    if (!conversation) this.store.failNotFound('会话不存在');

    const to = normalizeRecipients(cmd.to, this.store);
    const cc = normalizeRecipients(cmd.cc, this.store);
    const bcc = normalizeRecipients(cmd.bcc, this.store);
    if (to.length === 0 && cc.length === 0 && bcc.length === 0) {
      this.store.failValidation('请至少填写一位收件人');
    }

    const plain = stripHtml(cmd.body_html);
    if (!plain && cmd.attachments.length === 0) {
      this.store.failValidation('正文或附件不能为空');
    }

    let threadId: string | null = null;
    let subject = cmd.subject.trim();

    if (cmd.reply_to_message_id) {
      const replyKey = this.store.messageKey(
        cmd.account_id,
        cmd.reply_to_message_id,
      );
      const replyMsg = this.store.messages.get(replyKey);
      if (!replyMsg || replyMsg.sync_status === 'deleted_remote') {
        this.store.failNotFound('回复目标邮件不存在');
      }
      threadId = replyMsg.thread_id ?? this.store.nextId('thread');
      if (!subject) subject = replySubject(replyMsg.subject);
    }

    const t = Date.now();
    const messageId = this.store.nextId('msg') as MessageId;
    const message: Message = {
      id: messageId,
      account_id: cmd.account_id,
      graph_id: null,
      thread_id: threadId,
      graph_change_key: null,
      internet_message_id: null,
      conversation_index: null,
      subject,
      body_preview: plain.slice(0, 160),
      body_content_type: 'html',
      body_content: cmd.body_html,
      from_address: account.email,
      from_name: account.display_name || account.email,
      received_at: null,
      sent_at: t,
      created_at_graph: null,
      is_read: true,
      is_draft: false,
      is_outbound: true,
      has_attachments: cmd.attachments.length > 0,
      importance: 'normal',
      folder_id: null,
      parent_folder_name: '已发送',
      sync_status: 'pending_local',
      local_updated_at: t,
      remote_updated_at: null,
    };

    const recipients: MessageRecipient[] = [];
    const pushRole = (
      role: MessageRecipient['role'],
      list: ComposeRecipientRef[],
    ) => {
      list.forEach((r, seq) => {
        recipients.push({
          account_id: cmd.account_id,
          message_id: messageId,
          role,
          seq,
          normalized_address: r.address,
          display_name: r.display_name || r.address,
        });
      });
    };
    pushRole('to', to);
    pushRole('cc', cc);
    pushRole('bcc', bcc);

    const msgKey = this.store.messageKey(cmd.account_id, messageId);
    this.store.messages.set(msgKey, message);
    this.store.recipients.set(msgKey, recipients);

    const ids = this.store.conversationMessages.get(convKey) ?? [];
    ids.unshift(messageId);
    this.store.conversationMessages.set(convKey, ids);
    this.store.recomputeConversation(cmd.account_id, cmd.conversation_id);

    this.drafts.delete(this.draftKey(cmd.account_id, cmd.conversation_id));

    return { message, recipients };
  }
}
