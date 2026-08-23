import type { components } from '@/generated/api';

export type PersonalToken = components['schemas']['PersonalTokenResponse'];
export type TokenScope = components['schemas']['Scope'];

/** UI に並べるスコープの一覧。backend の `entity::scopes::Scope` と 1:1。 */
export const SCOPE_CATALOG: { scope: TokenScope; description: string }[] = [
  { scope: 'read:task', description: 'タスクとコメントの参照' },
  { scope: 'write:task', description: 'タスクの作成・編集・削除' },
  { scope: 'read:project', description: 'プロジェクトの参照' },
  { scope: 'write:project', description: 'プロジェクトの管理' },
  { scope: 'read:milestone', description: 'マイルストーンの参照' },
  { scope: 'write:milestone', description: 'マイルストーンの管理' },
  { scope: 'read:sprint', description: 'スプリントの参照' },
  { scope: 'write:sprint', description: 'スプリントの管理' },
  { scope: 'read:drive', description: 'ドライブのファイル参照' },
  { scope: 'write:drive', description: 'ドライブのファイル管理' },
  { scope: 'admin:tenant', description: 'テナント内のすべての操作（他のスコープを包含）' },
];

export const EXPIRATION_PRESETS = [
  { key: '30d', label: '30日', days: 30 },
  { key: '90d', label: '90日', days: 90 },
  { key: '1y', label: '1年', days: 365 },
  { key: 'none', label: '無期限', days: null },
] as const;

export type ExpirationKey = (typeof EXPIRATION_PRESETS)[number]['key'];

export function expiresAtFromPreset(key: ExpirationKey, now: Date = new Date()): string | null {
  const preset = EXPIRATION_PRESETS.find((p) => p.key === key);
  if (!preset || preset.days === null) return null;
  return new Date(now.getTime() + preset.days * 86400000).toISOString();
}

/** 平文は保存していないため、末尾 4 文字だけを添えた伏せ字で表示する。 */
export function maskedToken(lastFour: string): string {
  return `pat_••••••${lastFour}`;
}

export function formatExpiry(iso: string | null | undefined, now: Date = new Date()): string {
  if (!iso) return '無期限';
  const d = new Date(iso);
  const label = d.toLocaleDateString('ja-JP', {
    year: 'numeric',
    month: 'numeric',
    day: 'numeric',
  });
  return d.getTime() <= now.getTime() ? `${label} に期限切れ` : `${label} まで有効`;
}

export function formatLastUsed(iso: string | null | undefined, now: Date = new Date()): string {
  if (!iso) return '未使用';
  const diffMs = now.getTime() - new Date(iso).getTime();
  if (diffMs < 60_000) return 'たった今使用';
  const minutes = Math.floor(diffMs / 60_000);
  if (minutes < 60) return `${minutes}分前に使用`;
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return `${hours}時間前に使用`;
  const days = Math.floor(hours / 24);
  if (days < 30) return `${days}日前に使用`;
  const label = new Date(iso).toLocaleDateString('ja-JP', {
    year: 'numeric',
    month: 'numeric',
    day: 'numeric',
  });
  return `${label} に使用`;
}
