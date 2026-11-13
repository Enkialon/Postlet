import type { InjectionKey } from 'vue';
import type { AppPorts } from '../ports';
import type { NavigationApi } from './navigation';

export const portsKey: InjectionKey<AppPorts> = Symbol('app-ports');
export const navigationKey: InjectionKey<NavigationApi> = Symbol('navigation');
