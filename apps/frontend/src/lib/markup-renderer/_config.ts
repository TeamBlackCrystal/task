/**
 * _config.ts — KFM プロファイル設定の多層解決。Phase 1 はコード既定 ＋ system 層のみ。
 *
 * レイヤー・スタック (一般 → 具体・具体が勝つ):
 *   コード既定 (安全 universe・deploy 固定) → system。
 *   Phase 2 seam: tenant / project / user 層と lock (enforced) は resolveContentConfig の
 *   引数列へ層を足し、キー単位スパース上書きを重ねるだけで拡張する。
 *
 * どの層も、コードに定義済みの安全 universe (profile 列挙・トグル) から選ぶだけで、
 * 新しいタグ・属性・class を設定で持ち込むことはできない (sanitize registry が backstop)。
 * content-scope の解決結果はキャッシュキーに全文が焼き込まれ、設定変更で旧 HTML が
 * 自動失効する (_cache.ts)。viewer-scope (ユーザ個人設定) は HTML に焼かず CSS/client で
 * 当てる方針のためここには含めない。
 */
import type { KfmProfile } from './_renderer';

export type KfmContentConfig = {
  /** content source から profile を解決できない場合の既定 (保守的に github = 装飾なし) */
  readonly defaultProfile: KfmProfile;
};

const CODE_DEFAULTS: KfmContentConfig = {
  defaultProfile: 'github',
};

/** キー単位スパース上書き: 各層は触るキーだけ持ち、未設定キーは下層へ fall through */
export function resolveContentConfig(
  systemLayer: Partial<KfmContentConfig> = {},
): KfmContentConfig {
  const resolved: KfmContentConfig = { ...CODE_DEFAULTS };
  for (const key of Object.keys(systemLayer) as (keyof KfmContentConfig)[]) {
    const value = systemLayer[key];
    if (value !== undefined) {
      (resolved as Record<string, unknown>)[key] = value;
    }
  }
  return resolved;
}
