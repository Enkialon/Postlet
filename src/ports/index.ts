export type { AccountPort } from './accountPort';
export type { SyncPort } from './syncPort';
export type { ContactPort } from './contactPort';
export type { ConversationPort } from './conversationPort';
export type { MessagePort } from './messagePort';
export type { GroupPort } from './groupPort';
export type { PlatformPort } from './platformPort';

import type { AccountPort } from './accountPort';
import type { SyncPort } from './syncPort';
import type { ContactPort } from './contactPort';
import type { ConversationPort } from './conversationPort';
import type { MessagePort } from './messagePort';
import type { GroupPort } from './groupPort';
import type { PlatformPort } from './platformPort';

/** 组合根注入的全部入站端口 */
export interface AppPorts {
  account: AccountPort;
  sync: SyncPort;
  contact: ContactPort;
  conversation: ConversationPort;
  message: MessagePort;
  group: GroupPort;
  platform: PlatformPort;
}
