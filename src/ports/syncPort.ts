import type {
  ContactSyncProgress,
  MailSyncProgress,
  TriggerContactSyncCommand,
  TriggerMailSyncCommand,
} from '../domain/dto';
import type { AccountId } from '../domain/types';

/** 同步入站端口 */
export interface SyncPort {
  triggerMailSync(cmd: TriggerMailSyncCommand): Promise<void>;
  triggerContactSync(cmd: TriggerContactSyncCommand): Promise<void>;
  getMailSyncProgress(accountId: AccountId): Promise<MailSyncProgress>;
  getContactSyncProgress(accountId: AccountId): Promise<ContactSyncProgress>;
}
