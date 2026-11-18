import type { AppPorts } from '../../ports';
import type {
  ComposeDraft,
  ContactListQuery,
  ContactSyncProgress,
  CreateManualGroupCommand,
  CursorPage,
  DeleteMessagesCommand,
  ComposeAttachmentDraft,
  ComposeSendResult,
  HideGroupCommand,
  EnsureGroupSessionCommand,
  EnsurePrivateSessionCommand,
  GroupListQuery,
  ImportAttachmentCommand,
  MailSyncProgress,
  MarkMessagesReadCommand,
  MergeSessionsCommand,
  ReassignMessagesCommand,
  RenameGroupCommand,
  SaveComposeDraftCommand,
  SendComposedMailCommand,
  SessionListItem,
  SessionListQuery,
  SessionMessagesQuery,
  SetFolderSyncEnabledCommand,
  TriggerContactSyncCommand,
  TriggerMailSyncCommand,
  UpdateAccountAliasesCommand,
  UpdateAccountDisplaySettingsCommand,
  UpdateGroupSendMembersCommand,
  UpdateSentArchivePolicyCommand,
  UpsertImapSmtpAccountCommand,
} from '../../domain/dto';
import type {
  Account,
  Contact,
  Group,
  GroupParticipant,
  MailFolderSyncState,
  MessageWithRecipients,
  Session,
} from '../../domain/entities';
import type {
  AccountId,
  ContactId,
  EmailAddress,
  GroupId,
  MessageId,
  SendState,
  SessionId,
} from '../../domain/types';
import { invoke } from './invoke';
import { normalizeAppError } from './normalizeError';
import { TauriChangeEventsAdapter } from './TauriChangeEventsAdapter';
import { externalMailUrl } from '../../shared/mail-content/mailDocument';

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
      clearLocalData: (accountId: AccountId) =>
        invoke<void>('account_clear_local_data', { accountId }),
      updateDisplaySettings: (cmd: UpdateAccountDisplaySettingsCommand) =>
        invoke<Account>('update_display_settings', { cmd }),
      updateAliases: (cmd: UpdateAccountAliasesCommand) =>
        invoke<Account>('update_account_aliases', { cmd }),
      updateSentArchivePolicy: (cmd: UpdateSentArchivePolicyCommand) =>
        invoke<Account>('update_sent_archive_policy', { cmd }),
      getChangeVersion: (accountId: AccountId) =>
        invoke<number>('get_account_change_version', { accountId }),
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
    session: {
      listSessions: (query: SessionListQuery) =>
        invoke<CursorPage<SessionListItem>>('list_sessions', { query }),
      getSession: (accountId: AccountId, sessionId: SessionId) =>
        invoke<SessionListItem>('get_session', { accountId, sessionId }),
      listMessages: (query: SessionMessagesQuery) =>
        invoke<CursorPage<MessageWithRecipients>>('list_session_messages', {
          query,
        }),
      ensurePrivateSession: (cmd: EnsurePrivateSessionCommand) =>
        invoke<Session>('ensure_private_session', { cmd }),
      ensureGroupSession: (cmd: EnsureGroupSessionCommand) =>
        invoke<Session>('ensure_group_session', { cmd }),
      hideSession: (accountId: AccountId, sessionId: SessionId) =>
        invoke<void>('hide_session', { accountId, sessionId }),
      reassignMessages: (cmd: ReassignMessagesCommand) =>
        invoke<void>('reassign_messages', { cmd }),
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
      deleteMessages: (cmd: DeleteMessagesCommand) =>
        invoke<void>('delete_messages', { cmd }),
    },
    compose: {
      getDraft: (accountId: AccountId, sessionId: SessionId) =>
        invoke<ComposeDraft | null>('compose_get_draft', {
          accountId,
          sessionId,
        }),
      saveDraft: (cmd: SaveComposeDraftCommand) =>
        invoke<ComposeDraft>('compose_save_draft', { cmd }),
      clearDraft: (accountId: AccountId, sessionId: SessionId) =>
        invoke<void>('compose_clear_draft', { accountId, sessionId }),
      importAttachment: (cmd: ImportAttachmentCommand) =>
        invoke<ComposeAttachmentDraft>('compose_import_attachment', { cmd }),
      send: (cmd: SendComposedMailCommand) =>
        invoke<ComposeSendResult>('compose_send', { cmd }),
      getSendState: (accountId: AccountId, messageId: MessageId) =>
        invoke<SendState | null>('get_send_state', { accountId, messageId }),
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
      hideGroup: (cmd: HideGroupCommand) =>
        invoke<void>('hide_group', { cmd }),
      mergeSessions: (cmd: MergeSessionsCommand) =>
        invoke<Session>('merge_sessions', { cmd }),
      updateSendMembers: (cmd: UpdateGroupSendMembersCommand) =>
        invoke<GroupParticipant[]>('update_group_send_members', { cmd }),
    },
    platform: {
      openExternalUrl: async (url: string) => {
        const { openUrl } = await import('@tauri-apps/plugin-opener');
        const safeUrl = externalMailUrl(url);
        if (!safeUrl) {
          throw normalizeAppError({
            code: 'validation',
            message: '仅允许 http / https / mailto 外链',
            retryable: false,
          });
        }
        await openUrl(safeUrl);
      },
    },
    changeEvents: new TauriChangeEventsAdapter(),
  };
}
