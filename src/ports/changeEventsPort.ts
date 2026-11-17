/** 与 Rust `ChangeOrigin` / `MailChangedEvent` 对齐 */

export type ChangeOrigin =
  | 'initial_sync'
  | 'incremental_sync'
  | 'user_action'
  | 'rebuild';

/** UI 失效事件：不含邮件正文；收到后经 Port 重读本地库 */
export interface MailChangedEvent {
  account_id: string;
  generation: number;
  origin: ChangeOrigin;
  session_ids: string[];
  message_ids: string[];
  new_incoming_count: number;
  /** 账号本地聊天数据已整表清空 */
  account_cleared?: boolean;
}

export interface ChangeEventsPort {
  /**
   * 订阅本地变更失效事件。
   * @returns 取消订阅函数
   */
  subscribeMailChanged(
    handler: (event: MailChangedEvent) => void,
  ): Promise<() => void>;
}
