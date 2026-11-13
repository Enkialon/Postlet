import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import type {
  ChangeEventsPort,
  MailChangedEvent,
} from '../../ports/changeEventsPort';

const MAIL_CHANGED_EVENT = 'mail.changed';

/** Tauri：监听 Rust ChangeDispatcher 发出的 mail.changed */
export class TauriChangeEventsAdapter implements ChangeEventsPort {
  async subscribeMailChanged(
    handler: (event: MailChangedEvent) => void,
  ): Promise<() => void> {
    let lastGeneration = 0;
    const unlisten: UnlistenFn = await listen<MailChangedEvent>(
      MAIL_CHANGED_EVENT,
      (ev) => {
        const payload = ev.payload;
        if (!payload || typeof payload.generation !== 'number') return;
        if (payload.generation <= lastGeneration) return;
        lastGeneration = payload.generation;
        handler(payload);
      },
    );
    return () => {
      void unlisten();
    };
  }
}
