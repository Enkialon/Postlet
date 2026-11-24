import type {
  ContactSyncProgress,
  MailSyncProgress,
  SetFolderJunkOverrideCommand,
  SetFolderSyncEnabledCommand,
  TriggerContactSyncCommand,
  TriggerMailSyncCommand,
} from '../domain/dto';
import type { MailFolderSyncState } from '../domain/entities';
import type { AccountId } from '../domain/types';

/** 同步入站端口 */
export interface SyncPort {
  triggerMailSync(cmd: TriggerMailSyncCommand): Promise<void>;
  triggerContactSync(cmd: TriggerContactSyncCommand): Promise<void>;
  getMailSyncProgress(accountId: AccountId): Promise<MailSyncProgress>;
  getContactSyncProgress(accountId: AccountId): Promise<ContactSyncProgress>;
  /** 从远端发现文件夹树并写入本地元数据 */
  refreshMailFolders(accountId: AccountId): Promise<MailFolderSyncState[]>;
  /** 勾选/取消文件夹纳入账户全局同步 */
  setFolderSyncEnabled(
    cmd: SetFolderSyncEnabledCommand,
  ): Promise<MailFolderSyncState>;
  setFolderWatchEnabled(accountId: AccountId, folderId: string, enabled: boolean): Promise<MailFolderSyncState>;
  /** 保存文件夹垃圾邮件三态覆盖；不触发网络同步 */
  setFolderJunkOverride(
    cmd: SetFolderJunkOverrideCommand,
  ): Promise<MailFolderSyncState>;
}
