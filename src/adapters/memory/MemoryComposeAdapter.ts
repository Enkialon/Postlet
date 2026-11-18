import type { ComposePort } from '../../ports/composePort';
import type {
  ComposeAttachmentDraft,
  ComposeDraft,
  ComposeRecipientRef,
  ImportAttachmentCommand,
  SaveComposeDraftCommand,
  ComposeSendResult,
  SendComposedMailCommand,
} from '../../domain/dto';
import type { Message, MessageRecipient } from '../../domain/entities';
import type { AccountId, MessageId, SendState, SessionId } from '../../domain/types';
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

function decodeBase64(raw: string): Uint8Array | null {
  try {
    const binary = atob(raw.trim());
    const out = new Uint8Array(binary.length);
    for (let i = 0; i < binary.length; i++) out[i] = binary.charCodeAt(i);
    return out;
  } catch {
    return null;
  }
}

type StoredContent = {
  bytes: Uint8Array;
  byte_length: number;
  refs: Set<string>;
};

/** 内存写信适配器：与 Rust 同一契约（content_id 引用 + 导入挂草稿） */
export class MemoryComposeAdapter implements ComposePort {
  private readonly drafts = new Map<string, ComposeDraft>();
  private readonly contents = new Map<string, StoredContent>();

  constructor(private readonly store: MemoryStore) {}

  private draftKey(accountId: AccountId, sessionId: SessionId): string {
    return `${accountId}:${sessionId}`;
  }

  private contentKey(accountId: AccountId, contentId: string): string {
    return `${accountId}:${contentId}`;
  }

  private draftRefKey(accountId: AccountId, sessionId: SessionId): string {
    return `draft:${accountId}:${sessionId}`;
  }

  private ensureDraftShell(
    accountId: AccountId,
    sessionId: SessionId,
  ): ComposeDraft {
    const key = this.draftKey(accountId, sessionId);
    const existing = this.drafts.get(key);
    if (existing) return existing;
    const draft: ComposeDraft = {
      account_id: accountId,
      session_id: sessionId,
      reply_to_message_id: null,
      to: [],
      cc: [],
      bcc: [],
      subject: '',
      body_html: '',
      attachments: [],
      updated_at: Date.now(),
    };
    this.drafts.set(key, draft);
    return draft;
  }

  private releaseDraftRefs(accountId: AccountId, sessionId: SessionId): void {
    const refKey = this.draftRefKey(accountId, sessionId);
    for (const [ck, stored] of this.contents) {
      if (!ck.startsWith(`${accountId}:`)) continue;
      stored.refs.delete(refKey);
      if (stored.refs.size === 0) this.contents.delete(ck);
    }
  }

  async getDraft(
    accountId: AccountId,
    sessionId: SessionId,
  ): Promise<ComposeDraft | null> {
    this.store.requireAccount(accountId);
    const draft = this.drafts.get(this.draftKey(accountId, sessionId));
    if (!draft) return null;
    return {
      ...draft,
      to: draft.to.map((r) => ({ ...r })),
      cc: draft.cc.map((r) => ({ ...r })),
      bcc: draft.bcc.map((r) => ({ ...r })),
      attachments: draft.attachments.map((a) => ({ ...a })),
    };
  }

  async saveDraft(cmd: SaveComposeDraftCommand): Promise<ComposeDraft> {
    this.store.requireAccount(cmd.account_id);
    for (const att of cmd.attachments) {
      const stored = this.contents.get(
        this.contentKey(cmd.account_id, att.content_id),
      );
      if (!stored) {
        this.store.failValidation(`附件内容不存在: ${att.content_id}`);
      }
    }

    this.releaseDraftRefs(cmd.account_id, cmd.session_id);
    const refKey = this.draftRefKey(cmd.account_id, cmd.session_id);
    for (const att of cmd.attachments) {
      const stored = this.contents.get(
        this.contentKey(cmd.account_id, att.content_id),
      )!;
      stored.refs.add(refKey);
    }

    const draft: ComposeDraft = {
      account_id: cmd.account_id,
      session_id: cmd.session_id,
      reply_to_message_id: cmd.reply_to_message_id,
      to: normalizeRecipients(cmd.to, this.store),
      cc: normalizeRecipients(cmd.cc, this.store),
      bcc: normalizeRecipients(cmd.bcc, this.store),
      subject: cmd.subject,
      body_html: cmd.body_html,
      attachments: cmd.attachments.map((a) => ({ ...a })),
      updated_at: Date.now(),
    };
    this.drafts.set(this.draftKey(cmd.account_id, cmd.session_id), draft);
    return {
      ...draft,
      attachments: draft.attachments.map((a) => ({ ...a })),
    };
  }

  async clearDraft(accountId: AccountId, sessionId: SessionId): Promise<void> {
    this.store.requireAccount(accountId);
    this.releaseDraftRefs(accountId, sessionId);
    this.drafts.delete(this.draftKey(accountId, sessionId));
  }

  async importAttachment(
    cmd: ImportAttachmentCommand,
  ): Promise<ComposeAttachmentDraft> {
    this.store.requireAccount(cmd.account_id);
    const name = cmd.name.trim();
    if (!name) this.store.failValidation('附件名称不能为空');
    const bytes = decodeBase64(cmd.content_base64);
    if (!bytes) this.store.failValidation('附件内容 Base64 无效');
    if (bytes.length !== cmd.byte_length) {
      this.store.failValidation('附件长度与声明不一致');
    }
    const contentId = this.store.nextId('att');
    const att: ComposeAttachmentDraft = {
      content_id: contentId,
      name,
      size: bytes.length,
      mime: cmd.mime.trim() || 'application/octet-stream',
    };
    const draft = this.ensureDraftShell(cmd.account_id, cmd.session_id);
    const refKey = this.draftRefKey(cmd.account_id, cmd.session_id);
    this.contents.set(this.contentKey(cmd.account_id, contentId), {
      bytes,
      byte_length: bytes.length,
      refs: new Set([refKey]),
    });
    draft.attachments = [...draft.attachments, att];
    draft.updated_at = Date.now();
    this.drafts.set(this.draftKey(cmd.account_id, cmd.session_id), draft);
    return { ...att };
  }

  /** 内存适配器：入队后立即视为 queued；无真实远端 */
  private sendStates = new Map<string, SendState>();

  async getSendState(
    accountId: AccountId,
    messageId: MessageId,
  ): Promise<SendState | null> {
    return this.sendStates.get(`${accountId}:${messageId}`) ?? null;
  }

  async send(cmd: SendComposedMailCommand): Promise<ComposeSendResult> {
    if (cmd.attachments.length > 0) {
      this.store.failValidation(
        '附件内容存储尚未接通发送通道，无法发送含附件的邮件；草稿已保留',
      );
    }

    const account = this.store.requireAccount(cmd.account_id);
    const sessKey = this.store.sessionKey(cmd.account_id, cmd.session_id);
    const session = this.store.sessions.get(sessKey);
    if (!session || session.merged_into_id) this.store.failNotFound('会话不存在');

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

    let inReplyTo: string | null = null;
    let subject = cmd.subject.trim();

    if (cmd.reply_to_message_id) {
      const replyKey = this.store.messageKey(
        cmd.account_id,
        cmd.reply_to_message_id,
      );
      const replyMsg = this.store.messages.get(replyKey);
      if (!replyMsg) {
        this.store.failNotFound('回复目标邮件不存在');
      }
      inReplyTo =
        replyMsg.internet_message_id ??
        `local:${replyMsg.id}`;
      if (!subject) subject = replySubject(replyMsg.subject);
    }

    const t = Date.now();
    const messageId = this.store.nextId('msg') as MessageId;
    const message: Message = {
      id: messageId,
      account_id: cmd.account_id,
      session_id: cmd.session_id,
      remote_key: null,
      location_status: null,
      location_revision: 0,
      internet_message_id: null,
      in_reply_to: inReplyTo,
      subject,
      body_preview: plain.slice(0, 160),
      body_content_type: 'html',
      body_content: cmd.body_html,
      from_address: account.email,
      from_name: account.display_name || account.email,
      received_at: null,
      sent_at: t,
      is_read: true,
      is_draft: false,
      remote_is_read: null,
      remote_is_draft: null,
      remote_is_deleted: null,
      is_outbound: true,
      has_attachments: cmd.attachments.length > 0,
      importance: 'normal',
      folder_id: null,
      parent_folder_name: '已发送',
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
    this.store.recomputeSession(cmd.account_id, cmd.session_id);
    this.releaseDraftRefs(cmd.account_id, cmd.session_id);
    this.drafts.delete(this.draftKey(cmd.account_id, cmd.session_id));

    this.sendStates.set(`${cmd.account_id}:${messageId}`, 'queued');
    this.store.sendStates.set(
      this.store.messageKey(cmd.account_id, messageId),
      'queued',
    );
    return {
      message: { message, recipients },
      send_state: 'queued',
    };
  }
}
