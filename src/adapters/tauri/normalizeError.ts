export { normalizeAppError } from '../../shared/errors/normalizeAppError';

import { normalizeAppError } from '../../shared/errors/normalizeAppError';

export function rejectAsAppError(error: unknown): never {
  throw normalizeAppError(error);
}
