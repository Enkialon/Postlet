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
  const loadedConversationId = ref<string | null>(null);

  let saveTimer: ReturnType<typeof setTimeout> | null = null;

  const accountId = computed(() => nav.accountId.value);
  const conversationId = computed(() => nav.conversationId.value);

  async function defaultToRecipients(): Promise<ComposeRecipientRef[]> {
    const item = selected.value;
    const aid = accountId.value;
    if (!item || !aid) return [];

    if (
      item.conversation.kind === 'contact' &&
      item.conversation.normalized_address
    ) {
      const address = item.conversation.normalized_address;
      const name =
        item.contact_display_name?.trim() ||
        (await ports.contact
          .getContactByAddress(aid, address)
          .then((c) => c?.display_name?.trim() || '')
          .catch(() => '')) ||
        address;
      return [{ address, display_name: name }];
    }

    if (item.conversation.kind === 'group' && item.conversation.group_id) {
      try {
        const participants = await ports.group.listParticipants(
          aid,
          item.conversation.group_id,
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
    const cid = conversationId.value;
    if (!aid || !cid) {
      await applyDraft(null);
      loadedConversationId.value = null;
      return '';
    }
    try {
      const draft = await ports.compose.getDraft(aid, cid);
      await applyDraft(draft);
      loadedConversationId.value = cid;
      return draft?.body_html ?? '';
    } catch (e) {
      error.value = asAppError(e).message;
      await applyDraft(null);
      loadedConversationId.value = cid;
      return '';
    }
  }

  function scheduleSave(bodyHtml: string): void {
    const aid = accountId.value;
    const cid = conversationId.value;
    if (!aid || !cid) return;
    if (saveTimer) clearTimeout(saveTimer);
    saveTimer = setTimeout(() => {
      void persist(bodyHtml);
    }, 400);
  }

  async function persist(bodyHtml: string): Promise<void> {
    const aid = accountId.value;
    const cid = conversationId.value;
    if (!aid || !cid) return;
    saving.value = true;
    try {
      await ports.compose.saveDraft({
        account_id: aid,
        conversation_id: cid,
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

  function addAttachments(files: FileList | File[]): void {
    const list = Array.from(files);
    for (const file of list) {
      fields.attachments.push({
        id: `att-${Math.random().toString(36).slice(2, 10)}`,
        name: file.name,
        size: file.size,
        mime: file.type || 'application/octet-stream',
      });
    }
  }

  function removeAttachment(id: string): void {
    fields.attachments = fields.attachments.filter((a) => a.id !== id);
  }

  async function send(bodyHtml: string): Promise<MessageWithRecipients | null> {
    const aid = accountId.value;
    const cid = conversationId.value;
    if (!aid || !cid) return null;

    let to = cloneRecipients(fields.to);
    if (to.length === 0) to = await defaultToRecipients();

    sending.value = true;
    error.value = null;
    try {
      const result = await ports.compose.send({
        account_id: aid,
        conversation_id: cid,
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
    () => [accountId.value, conversationId.value] as const,
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
    loadedConversationId,
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
