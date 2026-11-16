import type { AppPorts } from '../../ports';
import { MemoryStore } from './memoryStore';
import { MemoryAccountAdapter } from './MemoryAccountAdapter';
import { MemorySyncAdapter } from './MemorySyncAdapter';
import { MemoryContactAdapter } from './MemoryContactAdapter';
import { MemoryConversationAdapter } from './MemoryConversationAdapter';
import { MemoryMessageAdapter } from './MemoryMessageAdapter';
import { MemoryComposeAdapter } from './MemoryComposeAdapter';
import { MemoryGroupAdapter } from './MemoryGroupAdapter';
import { MemoryPlatformAdapter } from './MemoryPlatformAdapter';
import { MemoryChangeEventsAdapter } from './MemoryChangeEventsAdapter';

/** 创建空的内存 Port 集（浏览器非 Tauri 回退） */
export function createMemoryPorts(store = new MemoryStore()): AppPorts {
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
