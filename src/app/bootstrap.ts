import type { App } from 'vue';
import { isTauri } from '@tauri-apps/api/core';
import type { AppPorts } from '../ports';
import { createMemoryPorts } from '../adapters/memory/createMemoryPorts';
import { createTauriPorts } from '../adapters/tauri/createTauriPorts';
import { portsKey, navigationKey } from './provide';
import { useNavigation } from './navigation';
import { i18n } from '../i18n';

export interface BootstrapOptions {
  /** 显式注入优先；否则桌面壳用 Tauri，浏览器用 Memory */
  ports?: AppPorts;
}

/** 组合根：创建 Adapter 并以 Port 注入 */
export function bootstrapApp(app: App, options: BootstrapOptions = {}): AppPorts {
  const ports =
    options.ports ??
    (isTauri() ? createTauriPorts() : createMemoryPorts({ seed: true }));
  const navigation = useNavigation();

  app.use(i18n);
  app.provide(portsKey, ports);
  app.provide(navigationKey, navigation);

  return ports;
}
