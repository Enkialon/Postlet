import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import type {
  ChangeEventsPort,
  MailChangedEvent,
} from '../../ports/changeEventsPort';

const MAIL_CHANGED_EVENT = 'mail-changed';

/** Tauri：监听 Rust ChangeDispatcher 发出的 mail-changed */
export class TauriChangeEventsAdapter implements ChangeEventsPort {
  async subscribeMailChanged(
    handler: (event: MailChangedEvent) => void,
  ): Promise<() => void> {
    const applied = new Map<string, number>();
    const unlisten: UnlistenFn = await listen<MailChangedEvent>(
      MAIL_CHANGED_EVENT,
      (ev) => {
        const payload = ev.payload;
        if (!payload || typeof payload.account_version !== 'number') return;
        const last = applied.get(payload.account_id) ?? 0;
        if (payload.account_version <= last) return;
        applied.set(payload.account_id, payload.account_version);
        handler(payload);
      },
    );
    return () => {
      void unlisten();
    };
  }
}
