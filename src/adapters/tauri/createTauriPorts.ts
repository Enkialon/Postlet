import type { AppPorts } from '../../ports';
import type {
  ComposeDraft,
  ContactListQuery,
  ContactSyncProgress,
  ConversationListItem,
  ConversationListQuery,
  ConversationMessagesQuery,
  CreateManualGroupCommand,
  CursorPage,
  EnsureContactConversationCommand,
  EnsureGroupConversationCommand,
  GroupListQuery,
  MailSyncProgress,
  MarkMessagesReadCommand,
  MergeGroupsCommand,
  MoveThreadToGroupCommand,
  RenameGroupCommand,
  SaveComposeDraftCommand,
  SendComposedMailCommand,
  SetFolderSyncEnabledCommand,
  TriggerContactSyncCommand,
  TriggerMailSyncCommand,
  UpdateAccountDisplaySettingsCommand,
  UpdateGroupSendMembersCommand,
  UpsertImapSmtpAccountCommand,
} from '../../domain/dto';
import type {
  Account,
  Contact,
  Conversation,
  Group,
  GroupParticipant,
  MailFolderSyncState,
  MessageWithRecipients,
} from '../../domain/entities';
import type {
  AccountId,
  ContactId,
  ConversationId,
  EmailAddress,
  GroupId,
  MessageId,
} from '../../domain/types';
import { invoke } from './invoke';
import { normalizeAppError } from './normalizeError';
import { TauriChangeEventsAdapter } from './TauriChangeEventsAdapter';

/**
 * Tauri Adapter：已注册 IPC → invoke。
 *
 * 注意：Tauri 2 命令**参数名**默认 camelCase（accountId）；
 * 嵌套 DTO 字段仍按 serde snake_case（account_id）。
 */
export function createTauriPorts(): AppPorts {
  return {
    account: {
      listAccounts: () => invoke<Account[]>('list_accounts'),
      getAccount: (accountId: AccountId) =>
        invoke<Account>('get_account', { accountId }),
      signIn: () => invoke<Account>('account_sign_in'),
      upsertImapSmtpAccount: (cmd: UpsertImapSmtpAccountCommand) =>
        invoke<Account>('upsert_imap_smtp_account', { cmd }),
      signOut: (accountId: AccountId) =>
        invoke<void>('account_sign_out', { accountId }),
      updateDisplaySettings: (cmd: UpdateAccountDisplaySettingsCommand) =>
        invoke<Account>('update_display_settings', { cmd }),
    },
    sync: {
      triggerMailSync: (cmd: TriggerMailSyncCommand) =>
        invoke<void>('trigger_mail_sync', { cmd }),
      triggerContactSync: (cmd: TriggerContactSyncCommand) =>
        invoke<void>('trigger_contact_sync', { cmd }),
      getMailSyncProgress: (accountId: AccountId) =>
        invoke<MailSyncProgress>('get_mail_sync_progress', { accountId }),
      getContactSyncProgress: (accountId: AccountId) =>
        invoke<ContactSyncProgress>('get_contact_sync_progress', {
          accountId,
        }),
      refreshMailFolders: (accountId: AccountId) =>
        invoke<MailFolderSyncState[]>('refresh_mail_folders', { accountId }),
      setFolderSyncEnabled: (cmd: SetFolderSyncEnabledCommand) =>
        invoke<MailFolderSyncState>('set_folder_sync_enabled', { cmd }),
    },
    contact: {
      listContacts: (query: ContactListQuery) =>
        invoke<CursorPage<Contact>>('list_contacts', { query }),
      getContact: (accountId: AccountId, contactId: ContactId) =>
        invoke<Contact>('get_contact', { accountId, contactId }),
      getContactByAddress: (accountId: AccountId, address: EmailAddress) =>
        invoke<Contact | null>('get_contact_by_address', {
          accountId,
          address,
        }),
    },
    conversation: {
      listConversations: (query: ConversationListQuery) =>
        invoke<CursorPage<ConversationListItem>>('list_conversations', {
          query,
        }),
      getConversation: (accountId: AccountId, conversationId: ConversationId) =>
        invoke<ConversationListItem>('get_conversation', {
          accountId,
          conversationId,
        }),
      listMessages: (query: ConversationMessagesQuery) =>
        invoke<CursorPage<MessageWithRecipients>>(
          'list_conversation_messages',
          { query },
        ),
      ensureContactConversation: (cmd: EnsureContactConversationCommand) =>
        invoke<Conversation>('ensure_contact_conversation', { cmd }),
      ensureGroupConversation: (cmd: EnsureGroupConversationCommand) =>
        invoke<Conversation>('ensure_group_conversation', { cmd }),
      hideConversation: (accountId: AccountId, conversationId: ConversationId) =>
        invoke<void>('hide_conversation', { accountId, conversationId }),
    },
    message: {
      getMessage: (accountId: AccountId, messageId: MessageId) =>
        invoke<MessageWithRecipients>('get_message', {
          accountId,
          messageId,
        }),
      ensureMessageBody: (accountId: AccountId, messageId: MessageId) =>
        invoke<MessageWithRecipients>('ensure_message_body', {
          accountId,
          messageId,
        }),
      markRead: (cmd: MarkMessagesReadCommand) =>
        invoke<void>('mark_messages_read', { cmd }),
    },
    compose: {
      getDraft: (accountId: AccountId, conversationId: ConversationId) =>
        invoke<ComposeDraft | null>('compose_get_draft', {
          accountId,
          conversationId,
        }),
      saveDraft: (cmd: SaveComposeDraftCommand) =>
        invoke<ComposeDraft>('compose_save_draft', { cmd }),
      clearDraft: (accountId: AccountId, conversationId: ConversationId) =>
        invoke<void>('compose_clear_draft', {
          accountId,
          conversationId,
        }),
      send: (cmd: SendComposedMailCommand) =>
        invoke<MessageWithRecipients>('compose_send', { cmd }),
    },
    group: {
      listGroups: (query: GroupListQuery) =>
        invoke<CursorPage<Group>>('list_groups', { query }),
      getGroup: (accountId: AccountId, groupId: GroupId) =>
        invoke<Group>('get_group', { accountId, groupId }),
      listParticipants: (accountId: AccountId, groupId: GroupId) =>
        invoke<GroupParticipant[]>('list_group_participants', {
          accountId,
          groupId,
        }),
      createManualGroup: (cmd: CreateManualGroupCommand) =>
        invoke<Group>('create_manual_group', { cmd }),
      renameGroup: (cmd: RenameGroupCommand) =>
        invoke<Group>('rename_group', { cmd }),
      mergeGroups: (cmd: MergeGroupsCommand) =>
        invoke<Group>('merge_groups', { cmd }),
      moveThreadToGroup: (cmd: MoveThreadToGroupCommand) =>
        invoke<void>('move_thread_to_group', { cmd }),
      updateSendMembers: (cmd: UpdateGroupSendMembersCommand) =>
        invoke<GroupParticipant[]>('update_group_send_members', { cmd }),
    },
    platform: {
      openExternalUrl: async (url: string) => {
        const { openUrl } = await import('@tauri-apps/plugin-opener');
        if (!/^https?:\/\//i.test(url.trim())) {
          throw normalizeAppError({
            code: 'validation',
            message: '仅允许 http / https 外链',
            retryable: false,
          });
        }
        await openUrl(url.trim());
      },
    },
    changeEvents: new TauriChangeEventsAdapter(),
  };
}
