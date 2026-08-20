import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { describe, expect, it } from 'vitest';
import { KFM_CONTENT_CLASS } from '../remark-gfm/content-class';

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const CSS_PATH = path.join(__dirname, '../remark-gfm/style.css');

/**
 * GFM サイドカー CSS の消費契約の機構化。
 *
 * GFM CSS は「明示 import ＋ v-html の器へ KFM_CONTENT_CLASS を付与」の二点契約
 * (style.css 冒頭)。器クラスの単一ソースは content-class.ts で、story の器は
 * これを import して使う。CSS 側だけが別名に改名される・bare 要素ルールが足されて
 * アプリ全体へ漏れる、の両方をここで弾く。
 *
 * パーサは素朴 (コメント除去 → `}` 分割 → `{` 前をセレクタとしてカンマ分解)。
 * @media 等の入れ子ルールは現状使っておらず、足すとここが赤くなる (fail-closed)。
 * その時はこのパーサごと更新すること。
 */

/** コメントを剥いだ CSS からルールセレクタ (カンマ分解済み) を列挙する */
const extractSelectors = (css: string): string[] =>
  css
    .replace(/\/\*[\s\S]*?\*\//g, '')
    .split('}')
    .map((block) => block.split('{')[0]?.trim() ?? '')
    .filter((selector) => selector.length > 0)
    .flatMap((selector) => selector.split(','))
    .map((selector) => selector.trim())
    .filter((selector) => selector.length > 0);

/** .kfm-content の子孫または子結合子だけを許す (兄弟結合子や at-rule は不可) */
const isScoped = (selector: string): boolean => {
  if (selector.includes('@')) return false;

  const scopeClass = new RegExp(`\\.${KFM_CONTENT_CLASS}(?![\\w-])`, 'g');
  return Array.from(selector.matchAll(scopeClass)).some((match) => {
    const suffix = selector.slice((match.index ?? 0) + match[0].length);
    // scope class と同じ compound の残り (.foo / :hover 等) を読み飛ばし、
    // 最初の combinator を検査する。現契約では descendant と child だけを許す。
    const relation = suffix.replace(/^[^\s>+~,{]*/, '');
    const trimmed = relation.trimStart();
    if (trimmed.startsWith('>')) return /^>\s*[^+~>\s]/.test(trimmed);
    return relation.length > trimmed.length && trimmed.length > 0 && !/^[+~>]/.test(trimmed);
  });
};

describe('GFM サイドカー CSS の消費契約 (scope 一致の機構)', () => {
  it('KFM_CONTENT_CLASS は文書化された値 kfm-content である', () => {
    // 器クラスは docs スニペットと消費側 .vue テンプレートに文字列として書かれる。
    // 値を変える = 既存消費側から CSS が黙って外れる破壊的変更なので、
    // この試験を意図的に触らせる関門にする。
    expect(KFM_CONTENT_CLASS).toBe('kfm-content');
  });

  it('検査器の陽性対照: 子孫・子結合子だけを許し、兄弟結合子と at-rule を拒む', () => {
    expect(isScoped('ul')).toBe(false);
    expect(isScoped(`.${KFM_CONTENT_CLASS}`)).toBe(false);
    expect(isScoped(`.${KFM_CONTENT_CLASS} ul`)).toBe(true);
    expect(isScoped(`.${KFM_CONTENT_CLASS} > ul`)).toBe(true);
    expect(isScoped(`.dark .${KFM_CONTENT_CLASS} a`)).toBe(true);
    expect(isScoped(`.${KFM_CONTENT_CLASS}-like ul`)).toBe(false);
    expect(isScoped(`.${KFM_CONTENT_CLASS} + ul`)).toBe(false);
    expect(isScoped(`.${KFM_CONTENT_CLASS} ~ ul`)).toBe(false);
    expect(isScoped(`@media print { .${KFM_CONTENT_CLASS} ul`)).toBe(false);
  });

  it('style.css の全ルールが器クラス子孫限定 (bare 要素への漏れ・片側改名を弾く)', () => {
    const selectors = extractSelectors(fs.readFileSync(CSS_PATH, 'utf8'));
    // 空振りだけを防ぐ。ルール削除を一律に破壊扱いせず、残った全ルールの scope を検査する。
    expect(selectors.length).toBeGreaterThanOrEqual(1);
    const unscoped = selectors.filter((selector) => !isScoped(selector));
    expect(unscoped).toEqual([]);
  });
});
