import type { Conversation, MailFolderSyncState } from './entities';
import type {
  AccountId,
  ConversationId,
  ConversationKind,
  EmailAddress,
  FolderId,
  GroupId,
  Importance,
  MailAuthKind,
  MailTlsMode,
  MessageId,
  PageDirection,
  SyncStatus,
  ThreadId,
  TimestampMs,
} from './types';

/** 游标分页结果 */
export interface CursorPage<T> {
  items: T[];
  next_cursor: string | null;
  has_more: boolean;
}

/** 会话列表查询；account_id 为空时跨账号 */
export interface ConversationListQuery {
  account_id: AccountId | null;
  cursor: string | null;
  limit: number;
  /** 可选：仅 contact / 仅 group */
  kind: ConversationKind | null;
}

/** 会话内消息查询 */
export interface ConversationMessagesQuery {
  account_id: AccountId;
  conversation_id: ConversationId;
  cursor: string | null;
  limit: number;
  /** 默认 older */
  direction: PageDirection;
}

/** 联系人列表查询；account_id 为空时跨账号 */
export interface ContactListQuery {
  account_id: AccountId | null;
  cursor: string | null;
  limit: number;
  /** 显示名 / 地址前缀 */
  search: string | null;
}

/** 群组列表查询；account_id 为空时跨账号 */
export interface GroupListQuery {
  account_id: AccountId | null;
  cursor: string | null;
  limit: number;
  /** 默认 false：只返回根群组（merged_into_id 为空） */
  include_merged: boolean;
}

/** 列表用消息摘要 */
export interface MessagePreview {
  id: MessageId;
  subject: string;
  body_preview: string;
  from_address: EmailAddress | null;
  from_name: string;
  received_at: TimestampMs | null;
  sent_at: TimestampMs | null;
  is_read: boolean;
  is_outbound: boolean;
  has_attachments: boolean;
  importance: Importance;
}

/** 会话列表项（含预览派生） */
export interface ConversationListItem {
  conversation: Conversation;
  /** 列表预览；软删消息不出现 */
  last_message_preview: MessagePreview | null;
  contact_display_name: string | null;
  group_name: string | null;
}

/** 登录后落库 / 刷新账户资料 */
export interface UpsertAccountCommand {
  id: AccountId;
  email: EmailAddress;
  display_name: string;
  aliases: EmailAddress[];
  tenant_id: string | null;
  home_account_id: string | null;
}

/** 更新账户显示配置 */
export interface UpdateAccountDisplaySettingsCommand {
  account_id: AccountId;
  show_group_messages_in_contact_conversation: boolean;
  /** 是否纳入「全部账号」跨账号列表 */
  include_in_all: boolean;
}

/** 新增 / 更新 IMAP+SMTP 账号（协议字段，非服务商品牌） */
export interface UpsertImapSmtpAccountCommand {
  email: string;
  display_name: string;
  auth_kind: MailAuthKind;
  imap_host: string;
  imap_port: number;
  imap_tls: MailTlsMode;
  smtp_host: string;
  smtp_port: number;
  smtp_tls: MailTlsMode;
  username: string | null;
  password: string | null;
  access_token: string | null;
  refresh_token: string | null;
  expires_at_ms: TimestampMs | null;
}

/** 批量标记已读 / 未读 */
export interface MarkMessagesReadCommand {
  account_id: AccountId;
  message_ids: MessageId[];
  is_read: boolean;
}

/** 用户空手创建群组 */
export interface CreateManualGroupCommand {
  account_id: AccountId;
  name: string;
  send_member_addresses: EmailAddress[];
}

/** 重命名群组（同时标记 name_is_manual） */
export interface RenameGroupCommand {
  account_id: AccountId;
  group_id: GroupId;
  name: string;
}

/** 合并群组：source 吸收进 target（存活根） */
export interface MergeGroupsCommand {
  account_id: AccountId;
  source_group_id: GroupId;
  target_group_id: GroupId;
}

/** 把线程挪到另一群组 */
export interface MoveThreadToGroupCommand {
  account_id: AccountId;
  thread_id: ThreadId;
  target_group_id: GroupId;
}

/** 更新发信成员集合 */
export interface UpdateGroupSendMembersCommand {
  account_id: AccountId;
  group_id: GroupId;
  send_member_addresses: EmailAddress[];
  /** true：锁定为手动，观测不再覆盖发信集合 */
  is_manual: boolean;
}

/** 确保存在联系人会话（可顺带占位联系人） */
export interface EnsureContactConversationCommand {
  account_id: AccountId;
  normalized_address: EmailAddress;
  display_name: string | null;
}

/** 确保存在群组会话（解析到存活根群组） */
export interface EnsureGroupConversationCommand {
  account_id: AccountId;
  group_id: GroupId;
}

/** 邮件同步进度读模型 */
export interface MailSyncProgress {
  account_id: AccountId;
  mail_sync_status: SyncStatus;
  mail_sync_error: string | null;
  mail_sync_triggered_at: TimestampMs | null;
  folders: MailFolderSyncState[];
}

/** 联系人同步进度读模型 */
export interface ContactSyncProgress {
  account_id: AccountId;
  contact_synced_at: TimestampMs | null;
  contact_delta_link: string | null;
}

/** 触发邮件同步 */
export interface TriggerMailSyncCommand {
  account_id: AccountId;
  /** null 则使用当前跟踪文件夹集合 */
  folder_ids: FolderId[] | null;
}

/** 触发联系人同步 */
export interface TriggerContactSyncCommand {
  account_id: AccountId;
}

/** 设置文件夹是否纳入全局同步 */
export interface SetFolderSyncEnabledCommand {
  account_id: AccountId;
  folder_id: FolderId;
  sync_enabled: boolean;
}

/** 写信附件草稿（仅元数据，无真实上传） */
export interface ComposeAttachmentDraft {
  id: string;
  name: string;
  size: number;
  mime: string;
}

/** 写信收件人（标签展示名 + 地址） */
export interface ComposeRecipientRef {
  address: EmailAddress;
  display_name: string;
}

/** 会话级写信草稿 */
export interface ComposeDraft {
  account_id: AccountId;
  conversation_id: ConversationId;
  /** 显式回复目标；null = 新邮件，不挂旧线程 */
  reply_to_message_id: MessageId | null;
  to: ComposeRecipientRef[];
  cc: ComposeRecipientRef[];
  bcc: ComposeRecipientRef[];
  subject: string;
  body_html: string;
  attachments: ComposeAttachmentDraft[];
  updated_at: TimestampMs;
}

/** 保存写信草稿 */
export interface SaveComposeDraftCommand {
  account_id: AccountId;
  conversation_id: ConversationId;
  reply_to_message_id: MessageId | null;
  to: ComposeRecipientRef[];
  cc: ComposeRecipientRef[];
  bcc: ComposeRecipientRef[];
  subject: string;
  body_html: string;
  attachments: ComposeAttachmentDraft[];
}

/** 发送写信内容 */
export interface SendComposedMailCommand {
  account_id: AccountId;
  conversation_id: ConversationId;
  reply_to_message_id: MessageId | null;
  to: ComposeRecipientRef[];
  cc: ComposeRecipientRef[];
  bcc: ComposeRecipientRef[];
  subject: string;
  body_html: string;
  attachments: ComposeAttachmentDraft[];
}
