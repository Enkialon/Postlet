import { computed, inject, reactive, ref, watch } from 'vue';
import { navigationKey, portsKey } from '../../app/provide';
import type {
  ComposeAttachmentDraft,
  ComposeDraft,
  ComposeRecipientRef,
} from '../../domain/dto';
import type { MessageId } from '../../domain/types';
import type { MessageWithRecipients } from '../../domain/entities';
import { asAppError } from '../../shared/errors/asAppError';
import { useConversations } from '../conversations/useConversations';

function emptyFields() {
  return {
    replyToMessageId: null as MessageId | null,
    to: [] as ComposeRecipientRef[],
    cc: [] as ComposeRecipientRef[],
    bcc: [] as ComposeRecipientRef[],
    subject: '',
    attachments: [] as ComposeAttachmentDraft[],
  };
}

function cloneRecipients(list: ComposeRecipientRef[]): ComposeRecipientRef[] {
  return list.map((r) => ({
    address: r.address,
    display_name: r.display_name,
  }));
}

/** 会话级草稿字段 + 回复目标；正文由 Tiptap 持有 */
export function useComposeDraft() {
  const ports = inject(portsKey)!;
  const nav = inject(navigationKey)!;
  const { selected } = useConversations();

  const fields = reactive(emptyFields());
  const sending = ref(false);
  const saving = ref(false);
  const error = ref<string | null>(null);
  const loadedSessionId = ref<string | null>(null);

  let saveTimer: ReturnType<typeof setTimeout> | null = null;

  const accountId = computed(() => nav.accountId.value);
  const sessionId = computed(() => nav.sessionId.value);

  async function defaultToRecipients(): Promise<ComposeRecipientRef[]> {
    const item = selected.value;
    const aid = accountId.value;
    if (!item || !aid) return [];

    if (
      item.session.kind === 'private' &&
      item.session.normalized_address
    ) {
      const address = item.session.normalized_address;
      const name =
        item.contact_display_name?.trim() ||
        (await ports.contact
          .getContactByAddress(aid, address)
          .then((c) => c?.display_name?.trim() || '')
          .catch(() => '')) ||
        address;
      return [{ address, display_name: name }];
    }

    if (item.session.kind === 'group' && item.session.group_id) {
      try {
        const participants = await ports.group.listParticipants(
          aid,
          item.session.group_id,
        );
        const sendMembers = participants.filter((p) => p.is_send_member);
        const source = sendMembers.length > 0 ? sendMembers : participants;
        const seen = new Set<string>();
        const out: ComposeRecipientRef[] = [];
        for (const p of source) {
          const address = p.normalized_address.toLowerCase();
          if (!address || seen.has(address)) continue;
          seen.add(address);
          out.push({
            address,
            display_name: p.display_name?.trim() || address,
          });
        }
        return out;
      } catch {
        return [];
      }
    }

    return [];
  }

  async function applyDraft(draft: ComposeDraft | null): Promise<void> {
    if (draft) {
      fields.replyToMessageId = draft.reply_to_message_id;
      fields.to = cloneRecipients(draft.to);
      fields.cc = cloneRecipients(draft.cc);
      fields.bcc = cloneRecipients(draft.bcc);
      fields.subject = draft.subject;
      fields.attachments = draft.attachments.map((a) => ({ ...a }));
      // 旧草稿若收件人为空，补上会话默认
      if (fields.to.length === 0) {
        fields.to = await defaultToRecipients();
      }
      return;
    }
    Object.assign(fields, emptyFields());
    fields.to = await defaultToRecipients();
  }

  async function loadForConversation(): Promise<string> {
    const aid = accountId.value;
    const sid = sessionId.value;
    if (!aid || !sid) {
      await applyDraft(null);
      loadedSessionId.value = null;
      return '';
    }
    try {
      const draft = await ports.compose.getDraft(aid, sid);
      await applyDraft(draft);
      loadedSessionId.value = sid;
      return draft?.body_html ?? '';
    } catch (e) {
      error.value = asAppError(e).message;
      await applyDraft(null);
      loadedSessionId.value = sid;
      return '';
    }
  }

  function scheduleSave(bodyHtml: string): void {
    const aid = accountId.value;
    const sid = sessionId.value;
    if (!aid || !sid) return;
    if (saveTimer) clearTimeout(saveTimer);
    saveTimer = setTimeout(() => {
      void persist(bodyHtml);
    }, 400);
  }

  async function persist(bodyHtml: string): Promise<void> {
    const aid = accountId.value;
    const sid = sessionId.value;
    if (!aid || !sid) return;
    saving.value = true;
    try {
      await ports.compose.saveDraft({
        account_id: aid,
        session_id: sid,
        reply_to_message_id: fields.replyToMessageId,
        to: cloneRecipients(fields.to),
        cc: cloneRecipients(fields.cc),
        bcc: cloneRecipients(fields.bcc),
        subject: fields.subject,
        body_html: bodyHtml,
        attachments: fields.attachments.map((a) => ({ ...a })),
      });
      error.value = null;
    } catch (e) {
      error.value = asAppError(e).message;
    } finally {
      saving.value = false;
    }
  }

  async function beginReply(item: MessageWithRecipients): Promise<void> {
    fields.replyToMessageId = item.message.id;
    const sub = item.message.subject.trim();
    if (sub && !fields.subject.trim()) {
      fields.subject = /^re:\s+/i.test(sub) ? sub : `Re: ${sub}`;
    }
    if (fields.to.length === 0) {
      const from = item.message.from_address;
      if (from && !item.message.is_outbound) {
        fields.to = [
          {
            address: from,
            display_name: item.message.from_name?.trim() || from,
          },
        ];
      } else {
        fields.to = await defaultToRecipients();
      }
    }
  }

  function clearReplyTarget(): void {
    fields.replyToMessageId = null;
  }

  async function fileToBase64(file: File): Promise<string> {
    const buf = new Uint8Array(await file.arrayBuffer());
    let binary = '';
    const chunk = 0x8000;
    for (let i = 0; i < buf.length; i += chunk) {
      binary += String.fromCharCode(...buf.subarray(i, i + chunk));
    }
    return btoa(binary);
  }

  async function addAttachments(files: FileList | File[]): Promise<void> {
    const aid = accountId.value;
    const sid = sessionId.value;
    if (!aid || !sid) return;
    const list = Array.from(files);
    for (const file of list) {
      try {
        const content_base64 = await fileToBase64(file);
        const att = await ports.compose.importAttachment({
          account_id: aid,
          session_id: sid,
          name: file.name,
          mime: file.type || 'application/octet-stream',
          byte_length: file.size,
          content_base64,
        });
        fields.attachments.push({ ...att });
        error.value = null;
      } catch (e) {
        error.value = asAppError(e).message;
      }
    }
  }

  function removeAttachment(contentId: string): void {
    fields.attachments = fields.attachments.filter(
      (a) => a.content_id !== contentId,
    );
  }

  /** 返回入队结果（queued 不代表服务器已接受） */
  async function send(
    bodyHtml: string,
  ): Promise<import('../../domain/dto').ComposeSendResult | null> {
    const aid = accountId.value;
    const sid = sessionId.value;
    if (!aid || !sid) return null;

    let to = cloneRecipients(fields.to);
    if (to.length === 0) to = await defaultToRecipients();

    sending.value = true;
    error.value = null;
    try {
      const result = await ports.compose.send({
        account_id: aid,
        session_id: sid,
        reply_to_message_id: fields.replyToMessageId,
        to,
        cc: cloneRecipients(fields.cc),
        bcc: cloneRecipients(fields.bcc),
        subject: fields.subject,
        body_html: bodyHtml,
        attachments: fields.attachments.map((a) => ({ ...a })),
      });
      Object.assign(fields, emptyFields());
      fields.to = await defaultToRecipients();
      return result;
    } catch (e) {
      error.value = asAppError(e).message;
      return null;
    } finally {
      sending.value = false;
    }
  }

  watch(
    () => [accountId.value, sessionId.value] as const,
    () => {
      if (saveTimer) {
        clearTimeout(saveTimer);
        saveTimer = null;
      }
    },
  );

  return {
    fields,
    sending,
    saving,
    error,
    loadedSessionId,
    loadedConversationId: loadedSessionId,
    loadForConversation,
    scheduleSave,
    persist,
    beginReply,
    clearReplyTarget,
    addAttachments,
    removeAttachment,
    send,
    defaultToRecipients,
  };
}
