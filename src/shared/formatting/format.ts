import type { TimestampMs } from '../../domain/types';

/** 本地时区时间展示；缺失返回空串由调用方决定「时间未知」 */
export function formatTimestamp(
  value: TimestampMs | null | undefined,
  locale = 'zh-CN',
): string {
  if (value == null) return '';
  try {
    return new Intl.DateTimeFormat(locale, {
      year: 'numeric',
      month: 'short',
      day: 'numeric',
      hour: '2-digit',
      minute: '2-digit',
    }).format(new Date(value));
  } catch {
    return '';
  }
}

export function formatTimestampOrUnknown(
  value: TimestampMs | null | undefined,
  locale = 'zh-CN',
  unknownLabel = '时间未知',
): string {
  return formatTimestamp(value, locale) || unknownLabel;
}

export function displayNameOrAddress(
  name: string | null | undefined,
  address: string | null | undefined,
): string {
  const n = name?.trim();
  if (n) return n;
  return address?.trim() || '';
}

export function formatUnreadBadge(count: number): string {
  if (count <= 0) return '';
  if (count > 99) return '99+';
  return String(count);
}

export function truncateText(text: string, max = 80): string {
  const t = text.trim();
  if (t.length <= max) return t;
  return `${t.slice(0, max - 1)}…`;
}
