import type {
  AccountId,
  AggregateJobStatus,
  AggregationOutcome,
  BodyContentType,
  ContactId,
  ConversationId,
  ConversationKind,
  EmailAddress,
  FolderId,
  GraphImmutableId,
  GroupId,
  GroupSource,
  Importance,
  MessageId,
  MessageSyncStatus,
  RecipientRole,
  SyncStatus,
  ThreadId,
  TimestampMs,
} from './types';

/** Outlook 账户（租户根） */
export interface Account {
  id: AccountId;
  /** 主邮箱（规范化） */
  email: EmailAddress;
  display_name: string;
  /** self 额外身份；与 email 组成排除集合 */
  aliases: EmailAddress[];
  tenant_id: string | null;
  home_account_id: string | null;
  /** 联系人 Delta 游标 */
  contact_delta_link: string | null;
  /** 最近一次账户邮件同步轮次全部成功结束时间 */
  mail_synced_at: TimestampMs | null;
  /** 最近一次联系人同步成功结束时间 */
  contact_synced_at: TimestampMs | null;
  mail_sync_status: SyncStatus;
  /** 最近一次失败原因；成功时清空 */
  mail_sync_error: string | null;
  /** 群组消息是否同时出现在联系人会话投影（默认否） */
  show_group_messages_in_contact_conversation: boolean;
  /** 是否纳入「全部账号」列表查询（默认是；未退出也可关掉） */
  include_in_all: boolean;
  created_at: TimestampMs;
  updated_at: TimestampMs;
}

/** 按账户 + 文件夹保存的邮件增量游标（非文件夹导航模型） */
export interface MailFolderSyncState {
  account_id: AccountId;
  folder_id: FolderId;
  /** 最近完成轮次的增量游标 */
  delta_link: string | null;
  /** 当前未完成轮次的分页续传位置 */
  next_link: string | null;
  synced_at: TimestampMs | null;
  sync_status: SyncStatus;
  last_error: string | null;
  updated_at: TimestampMs;
}

/** 通讯录本地缓存 */
export interface Contact {
  id: ContactId;
  account_id: AccountId;
  /** 可空：允许仅按地址建的占位联系人 */
  graph_id: string | null;
  display_name: string;
  /** 主地址（账户内唯一） */
  normalized_address: EmailAddress;
  created_at: TimestampMs;
  updated_at: TimestampMs;
}

/** 邮件真相（Graph 同步落库）；会话归属不改写本实体 */
export interface Message {
  id: MessageId;
  account_id: AccountId;
  /** Graph ImmutableId；本地草稿可空 */
  graph_id: GraphImmutableId | null;
  /** Graph conversationId；联系人投影不要求非空，线程归组要求非空 */
  thread_id: ThreadId | null;
  graph_change_key: string | null;
  /** RFC Message-ID */
  internet_message_id: string | null;
  conversation_index: number[] | null;
  subject: string;
  body_preview: string;
  body_content_type: BodyContentType;
  /** null=未加载；空串=已加载且正文为空 */
  body_content: string | null;
  from_address: EmailAddress | null;
  from_name: string;
  received_at: TimestampMs | null;
  sent_at: TimestampMs | null;
  created_at_graph: TimestampMs | null;
  is_read: boolean;
  is_draft: boolean;
  /** 是否本账户发出（结合 self 集合判定后落库） */
  is_outbound: boolean;
  has_attachments: boolean;
  importance: Importance;
  /** 当前位置文件夹；非主导航 */
  folder_id: FolderId | null;
  parent_folder_name: string | null;
  sync_status: MessageSyncStatus;
  local_updated_at: TimestampMs;
  remote_updated_at: TimestampMs | null;
}

/** 收件人拆行 */
export interface MessageRecipient {
  account_id: AccountId;
  message_id: MessageId;
  role: RecipientRole;
  /** 同角色内顺序 */
  seq: number;
  normalized_address: EmailAddress;
  display_name: string;
}

/** 邮件 + 收件人读写聚合 */
export interface MessageWithRecipients {
  message: Message;
  recipients: MessageRecipient[];
}

/** 业务群组（自动 / 手动） */
export interface Group {
  id: GroupId;
  account_id: AccountId;
  name: string;
  /** 名称是否用户手动指定 */
  name_is_manual: boolean;
  source: GroupSource;
  /** 发信成员是否手动锁定 */
  send_members_is_manual: boolean;
  /** 被吸收时指向存活根群组 */
  merged_into_id: GroupId | null;
  created_at: TimestampMs;
  updated_at: TimestampMs;
}

/** 线程 → 群组主归属（账户内每个 thread 最多一行） */
export interface GroupThread {
  account_id: AccountId;
  group_id: GroupId;
  thread_id: ThreadId;
  linked_at: TimestampMs;
}

/** 群组成员投影（观测 + 发信） */
export interface GroupParticipant {
  account_id: AccountId;
  group_id: GroupId;
  normalized_address: EmailAddress;
  display_name: string;
  /** 是否由邮件往来观测得到 */
  is_observed: boolean;
  /** 是否发信成员 */
  is_send_member: boolean;
  first_seen_at: TimestampMs;
  last_seen_at: TimestampMs;
}

/** 会话列表缓存；归属真相在 group_thread + conversation_messages */
export interface Conversation {
  id: ConversationId;
  account_id: AccountId;
  kind: ConversationKind;
  /** kind=contact 时非空 */
  normalized_address: EmailAddress | null;
  /** 可空；删联系人时清空引用，保留会话 */
  contact_id: ContactId | null;
  /** kind=group 时非空 */
  group_id: GroupId | null;
  last_message_at: TimestampMs | null;
  /** 须指向当前附着的非软删消息 */
  last_message_id: MessageId | null;
  /** 仅统计当前会话内非软删且未读 */
  unread_count: number;
  created_at: TimestampMs;
  updated_at: TimestampMs;
}

/** 消息 ↔ 会话多对多附着 */
export interface ConversationMessageLink {
  account_id: AccountId;
  conversation_id: ConversationId;
  message_id: MessageId;
  attached_at: TimestampMs;
}

/** 聚合任务（失败恢复 / 租约） */
export interface ConversationAggregateJob {
  account_id: AccountId;
  message_id: MessageId;
  /** 每次有效消息变更重新入队时单调递增 */
  generation: number;
  outcome: AggregationOutcome | null;
  status: AggregateJobStatus;
  attempts: number;
  last_error: string | null;
  /** 最早可处理时间 */
  available_at: TimestampMs;
  updated_at: TimestampMs;
  /** processing 时非空 */
  lease_token: string | null;
  lease_expires_at: TimestampMs | null;
}
