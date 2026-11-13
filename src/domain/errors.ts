/** 前端可展示错误 */
export interface AppError {
  code:
    | 'not_found'
    | 'validation'
    | 'conflict'
    | 'auth'
    | 'sync'
    | 'network'
    | 'internal';
  message: string;
  retryable: boolean;
}
