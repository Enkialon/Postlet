import type { MailFolderSyncState, Session } from './entities';
import type {
  AccountId,
  EmailAddress,
  FolderId,
  GroupId,
  Importance,
  MailTlsMode,
  MessageId,
  PageDirection,
  SessionId,
  SessionKind,
  SyncStatus,
  TimestampMs,
} from './types';

/** 游标分页结果 */
export interface CursorPage<T> {
  items: T[];
  next_cursor: string | null;
  has_more: boolean;
}

/** 会话列表查询；account_id 为空时跨账号 */
export interface SessionListQuery {
  account_id: AccountId | null;
  cursor: string | null;
  limit: number;
  /** 可选：仅 private / 仅 group */
  kind: SessionKind | null;
  /** 是否包含默认排除的文件夹范围（Trash/Junk/Drafts/All） */
  include_excluded_folders?: boolean;
}

/** 会话内消息查询 */
export interface SessionMessagesQuery {
  account_id: AccountId;
  session_id: SessionId;
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
  /** 群名 / 成员昵称或地址前缀 */
  search: string | null;
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

/** 会话列表项（含预览与显示名） */
export interface SessionListItem {
  session: Session;
  /** 读时解析：私聊=联系人名或地址；群聊=群名 */
  display_name: string;
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
}

/** 更新账户显示配置 */
export interface UpdateAccountDisplaySettingsCommand {
  account_id: AccountId;
  /** 是否纳入「全部账号」跨账号列表 */
  include_in_all: boolean;
}

/** 手动维护账户别名（self 额外身份；不做 Send-As） */
export interface UpdateAccountAliasesCommand {
  account_id: AccountId;
  /** 原始地址列表；后端 trim + lowercase 去重 */
  aliases: string[];
}

/** IMAP 仅密码；无 OAuth2 半支持分支 */
export type ImapAuthenticationInput = {
  kind: 'password';
  username: string;
  password: string;
};

/** SMTP 默认 Shared；显式 Password 才单独登录 */
export type SmtpAuthenticationInput =
  | { kind: 'shared' }
  | { kind: 'password'; username: string; password: string };

/** 新增 / 更新 IMAP+SMTP 账号（协议字段，非服务商品牌） */
export interface UpsertImapSmtpAccountCommand {
  email: string;
  display_name: string;
  imap_host: string;
  imap_port: number;
  imap_tls: MailTlsMode;
  smtp_host: string;
  smtp_port: number;
  smtp_tls: MailTlsMode;
  imap_auth: ImapAuthenticationInput;
  smtp_auth: SmtpAuthenticationInput;
}

/** 批量标记已读 / 未读 */
export interface MarkMessagesReadCommand {
  account_id: AccountId;
  message_ids: MessageId[];
  is_read: boolean;
}

/** 批量删除邮件（本地软删 + 推远端） */
export interface DeleteMessagesCommand {
  account_id: AccountId;
  message_ids: MessageId[];
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

/** 隐藏群组（隐藏群聊 Session；不删邮件） */
export interface HideGroupCommand {
  account_id: AccountId;
  group_id: GroupId;
}

/** 合并群聊 Session：source → target（消息迁入 + soft-link） */
export interface MergeSessionsCommand {
  account_id: AccountId;
  source_session_id: SessionId;
  target_session_id: SessionId;
}

/** 将选中消息改挂到目标 Session（私聊 / 群聊均可） */
export interface ReassignMessagesCommand {
  account_id: AccountId;
  message_ids: MessageId[];
  target_session_id: SessionId;
}

/** 更新发信成员集合 */
export interface UpdateGroupSendMembersCommand {
  account_id: AccountId;
  group_id: GroupId;
  send_member_addresses: EmailAddress[];
  /** true：锁定为手动，观测不再覆盖发信集合 */
  is_manual: boolean;
}

/** 确保存在私聊 Session（可顺带占位联系人） */
export interface EnsurePrivateSessionCommand {
  account_id: AccountId;
  normalized_address: EmailAddress;
  display_name: string | null;
}

/** 确保存在群聊 Session */
export interface EnsureGroupSessionCommand {
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

/** 写信附件草稿（稳定 content_id 引用受管理内容） */
export interface ComposeAttachmentDraft {
  content_id: string;
  name: string;
  size: number;
  mime: string;
}

/** 导入附件到受管理存储并挂到会话草稿 */
export interface ImportAttachmentCommand {
  account_id: AccountId;
  session_id: SessionId;
  name: string;
  mime: string;
  /** 声明字节数，须与解码后长度一致 */
  byte_length: number;
  /** 标准 Base64 编码的文件内容 */
  content_base64: string;
}

/** 发送结果：入队状态 + 本地消息（queued 不等于已送达） */
export interface ComposeSendResult {
  message: import('./entities').MessageWithRecipients;
  send_state: import('./types').SendState;
}

/** 写信收件人（标签展示名 + 地址） */
export interface ComposeRecipientRef {
  address: EmailAddress;
  display_name: string;
}

/** 会话级写信草稿 */
export interface ComposeDraft {
  account_id: AccountId;
  session_id: SessionId;
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
  session_id: SessionId;
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
  session_id: SessionId;
  reply_to_message_id: MessageId | null;
  to: ComposeRecipientRef[];
  cc: ComposeRecipientRef[];
  bcc: ComposeRecipientRef[];
  subject: string;
  body_html: string;
  attachments: ComposeAttachmentDraft[];
}
