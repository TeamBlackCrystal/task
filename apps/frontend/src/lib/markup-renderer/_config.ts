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

// 各キーの安全 universe (コード定義済みの列挙)。層の値はここから選ぶだけで、
// universe 外の値・未知キーは持ち込めない。
const KNOWN_PROFILES: readonly KfmProfile[] = ['github'];

/**
 * キー単位スパース上書き: 各層は触るキーだけ持ち、未設定キーは下層へ fall through。
 * 層の内容は無検証コピーしない — 既知キーのみを universe 検証つきで個別に取り込む
 * (未知キーは黙って無視、universe 外の値は設定不備として fail-fast で throw)。
 */
export function resolveContentConfig(
  systemLayer: Partial<KfmContentConfig> = {},
): KfmContentConfig {
  if (
    systemLayer.defaultProfile !== undefined &&
    !KNOWN_PROFILES.includes(systemLayer.defaultProfile)
  ) {
    throw new Error(
      `[markup-renderer] defaultProfile "${String(systemLayer.defaultProfile)}" is not in ` +
        `the safe universe (${KNOWN_PROFILES.join(', ')})`,
    );
  }
  return {
    defaultProfile: systemLayer.defaultProfile ?? CODE_DEFAULTS.defaultProfile,
  };
}
