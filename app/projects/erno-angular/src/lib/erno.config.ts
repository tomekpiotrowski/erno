import { InjectionToken } from '@angular/core';

export interface ErnoConfig {
  baseUrl: string;
  wsUrl: string;
}

export const ERNO_CONFIG = new InjectionToken<ErnoConfig>('ErnoConfig');
