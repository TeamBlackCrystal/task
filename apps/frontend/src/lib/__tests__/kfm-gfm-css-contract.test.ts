import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { describe, expect, it } from 'vitest';
import { KFM_CONTENT_CLASS } from '../remark-gfm/content-class';

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const REMARK_CSS_PATHS = fs.globSync(path.join(__dirname, '../remark-*/style.css'));
// 自身が emit する名前空間を直接指す CSS だけを器 scope から除外する。
// 除外は無検査を意味しないため、プラグインごとに自前 namespace も宣言する。
// 新しい remark-* CSS は既定で器 scope の検査対象になり、未登録でも素通りしない。
const EMITTED_NAMESPACE_SCOPED_PLUGINS = new Map([['remark-koyori-alerts', 'kfm-alert']]);
const CONTAINER_SCOPED_CSS_PATHS = REMARK_CSS_PATHS.filter(
  (cssPath) => !EMITTED_NAMESPACE_SCOPED_PLUGINS.has(path.basename(path.dirname(cssPath))),
);
const EMITTED_NAMESPACE_SCOPED_CSS_PATHS = REMARK_CSS_PATHS.filter((cssPath) =>
  EMITTED_NAMESPACE_SCOPED_PLUGINS.has(path.basename(path.dirname(cssPath))),
);

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
    // :has(.kfm-content) / :not(.kfm-content) 内の一致は、その外側を scope しない。
    // マッチ位置までに閉じていない括弧があれば functional pseudo-class 内とみなす。
    const prefix = selector.slice(0, match.index ?? 0);
    const openParentheses = Array.from(prefix).reduce((depth, character) => {
      if (character === '(') return depth + 1;
      if (character === ')') return Math.max(0, depth - 1);
      return depth;
    }, 0);
    if (openParentheses > 0) return false;

    const suffix = selector.slice((match.index ?? 0) + match[0].length);
    // scope class と同じ compound の残り (.foo / :hover 等) を読み飛ばし、
    // 最初の combinator を検査する。現契約では descendant と child だけを許す。
    const relation = suffix.replace(/^[^\s>+~,{]*/, '');
    const trimmed = relation.trimStart();
    if (trimmed.startsWith('>')) return /^>\s*[^+~>\s]/.test(trimmed);
    return relation.length > trimmed.length && trimmed.length > 0 && !/^[+~>]/.test(trimmed);
  });
};

/** 任意の dark theme 祖先の後で、自身が emit する namespace class から始まること */
const isEmittedNamespaceScoped = (selector: string, namespaceClass: string): boolean =>
  new RegExp(`^(?:\\.dark\\s+)?\\.${namespaceClass}(?=$|--|__|[\\s.:#>+~\\[])`).test(selector);

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
    expect(isScoped(`body:has(.${KFM_CONTENT_CLASS}) ul`)).toBe(false);
    expect(isScoped(`@media print { .${KFM_CONTENT_CLASS} ul`)).toBe(false);
  });

  it('名前空間検査器の陽性対照: 自前 class で始まる selector だけを許す', () => {
    expect(isEmittedNamespaceScoped('.kfm-alert', 'kfm-alert')).toBe(true);
    expect(isEmittedNamespaceScoped('.kfm-alert--note .kfm-alert__title', 'kfm-alert')).toBe(true);
    expect(isEmittedNamespaceScoped('.dark .kfm-alert--note', 'kfm-alert')).toBe(true);
    expect(isEmittedNamespaceScoped('.kfm-alert-like', 'kfm-alert')).toBe(false);
    expect(isEmittedNamespaceScoped('blockquote .kfm-alert', 'kfm-alert')).toBe(false);
    expect(isEmittedNamespaceScoped('.dark blockquote .kfm-alert', 'kfm-alert')).toBe(false);
  });

  it('器 scope が必要な remark-*/style.css の全ルールが器クラス子孫限定', () => {
    const discoveredPlugins = REMARK_CSS_PATHS.map((cssPath) =>
      path.basename(path.dirname(cssPath)),
    );
    const containerScopedPlugins = CONTAINER_SCOPED_CSS_PATHS.map((cssPath) =>
      path.basename(path.dirname(cssPath)),
    );
    const classifiedPlugins = [
      ...EMITTED_NAMESPACE_SCOPED_PLUGINS.keys(),
      ...containerScopedPlugins,
    ];
    expect(classifiedPlugins.sort()).toEqual(discoveredPlugins.sort());

    const selectors = CONTAINER_SCOPED_CSS_PATHS.flatMap((cssPath) =>
      extractSelectors(fs.readFileSync(cssPath, 'utf8')).map((selector) => ({
        file: path.relative(__dirname, cssPath),
        selector,
      })),
    );
    // 空振りだけを防ぐ。ルール削除を一律に破壊扱いせず、残った全ルールの scope を検査する。
    expect(selectors.length).toBeGreaterThanOrEqual(1);
    const unscoped = selectors.filter(({ selector }) => !isScoped(selector));
    expect(unscoped).toEqual([]);
  });

  it('器 scope 免除サイドカーの全ルールが自身の namespace class から始まる', () => {
    for (const cssPath of EMITTED_NAMESPACE_SCOPED_CSS_PATHS) {
      const plugin = path.basename(path.dirname(cssPath));
      const namespaceClass = EMITTED_NAMESPACE_SCOPED_PLUGINS.get(plugin);
      expect(namespaceClass).toBeDefined();
      const selectors = extractSelectors(fs.readFileSync(cssPath, 'utf8'));
      expect(selectors.length).toBeGreaterThanOrEqual(1);
      const unscoped = selectors.filter(
        (selector) => !namespaceClass || !isEmittedNamespaceScoped(selector, namespaceClass),
      );
      expect(unscoped).toEqual([]);
    }
  });
});
