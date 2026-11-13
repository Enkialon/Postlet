import type { App } from 'vue';
import type { AppPorts } from '../ports';
import { createMemoryPorts } from '../adapters/memory/createMemoryPorts';
import { portsKey, navigationKey } from './provide';
import { useNavigation } from './navigation';
import { i18n } from '../i18n';

export interface BootstrapOptions {
  /** 默认 memory；后续可注入 createTauriPorts() */
  ports?: AppPorts;
}

/** 组合根：创建 Adapter 并以 Port 注入 */
export function bootstrapApp(app: App, options: BootstrapOptions = {}): AppPorts {
  const ports = options.ports ?? createMemoryPorts({ seed: true });
  const navigation = useNavigation();

  app.use(i18n);
  app.provide(portsKey, ports);
  app.provide(navigationKey, navigation);

  return ports;
}
