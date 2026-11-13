import type { AppPorts } from '../../ports';
import { MemoryStore, createSeededMemoryStore } from './memoryStore';
import { MemoryAccountAdapter } from './MemoryAccountAdapter';
import { MemorySyncAdapter } from './MemorySyncAdapter';
import { MemoryContactAdapter } from './MemoryContactAdapter';
import { MemoryConversationAdapter } from './MemoryConversationAdapter';
import { MemoryMessageAdapter } from './MemoryMessageAdapter';
import { MemoryComposeAdapter } from './MemoryComposeAdapter';
import { MemoryGroupAdapter } from './MemoryGroupAdapter';
import { MemoryPlatformAdapter } from './MemoryPlatformAdapter';
import { MemoryChangeEventsAdapter } from './MemoryChangeEventsAdapter';

/** 创建内存 Port 集；默认带演示数据 */
export function createMemoryPorts(options?: { seed?: boolean }): AppPorts {
  const store =
    options?.seed === false ? new MemoryStore() : createSeededMemoryStore();
  return {
    account: new MemoryAccountAdapter(store),
    sync: new MemorySyncAdapter(store),
    contact: new MemoryContactAdapter(store),
    conversation: new MemoryConversationAdapter(store),
    message: new MemoryMessageAdapter(store),
    compose: new MemoryComposeAdapter(store),
    group: new MemoryGroupAdapter(store),
    platform: new MemoryPlatformAdapter(),
    changeEvents: new MemoryChangeEventsAdapter(),
  };
}
