/** Unix 毫秒时间戳 */
export type TimestampMs = number;
/** 规范化邮箱 */
export type EmailAddress = string;
export type AccountId = string;
export type ContactId = string;
export type MessageId = string;
export type GroupId = string;
export type ConversationId = string;
export type ThreadId = string;
export type GraphImmutableId = string;
export type FolderId = string;

/** 账户 / 文件夹同步状态 */
export type SyncStatus = 'idle' | 'running' | 'error';
/** 单封邮件同步状态 */
export type MessageSyncStatus = 'synced' | 'pending_local' | 'deleted_remote';
export type BodyContentType = 'text' | 'html';
export type Importance = 'low' | 'normal' | 'high';
export type RecipientRole = 'to' | 'cc' | 'bcc' | 'reply_to';
/** 群组诞生方式 */
export type GroupSource = 'auto' | 'manual';
export type ConversationKind = 'contact' | 'group';
export type AggregateJobStatus =
  | 'pending'
  | 'processing'
  | 'completed'
  | 'retry'
  | 'failed';
export type AggregationOutcome = 'completed' | 'partial' | 'deferred' | 'skipped';
export type PageDirection = 'older' | 'newer';
