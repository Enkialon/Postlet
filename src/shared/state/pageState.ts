import type { AppError } from '../../domain/errors';

/** 纯前端分页状态，非 IPC DTO */
export interface PageState<T> {
  items: T[];
  nextCursor: string | null;
  hasMore: boolean;
  loading: 'idle' | 'initial' | 'more' | 'refresh';
  error: AppError | null;
  requestVersion: number;
}

export function createEmptyPageState<T>(): PageState<T> {
  return {
    items: [],
    nextCursor: null,
    hasMore: false,
    loading: 'idle',
    error: null,
    requestVersion: 0,
  };
}
