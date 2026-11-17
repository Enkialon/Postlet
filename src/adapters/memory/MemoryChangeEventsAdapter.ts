import type {
  ChangeEventsPort,
  MailChangedEvent,
} from '../../ports/changeEventsPort';

/** Memory：进程内可手动 publish，供测试 */
export class MemoryChangeEventsAdapter implements ChangeEventsPort {
  private handlers = new Set<(event: MailChangedEvent) => void>();

  async subscribeMailChanged(
    handler: (event: MailChangedEvent) => void,
  ): Promise<() => void> {
    this.handlers.add(handler);
    return () => {
      this.handlers.delete(handler);
    };
  }

  /** 测试用：模拟一次本地变更（调用方给出账号版本） */
  publish(event: MailChangedEvent): void {
    for (const h of this.handlers) {
      h(event);
    }
  }
}
