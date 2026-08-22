import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { describe, expect, it } from 'vitest';
import { KFM_CONTENT_CLASS } from '../remark-gfm/content-class';

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const SIDECAR_CSS_PATHS = [
  ...fs.globSync(path.join(__dirname, '../remark-*/style.css')),
  ...fs.globSync(path.join(__dirname, '../rehype-*/style.css')),
];
// bare 要素を描画するプラグインだけ器クラスを要する。自身が emit する名前空間を
// 直接指すサイドカーも明示分類し、remark-* だけを拾って rehype-* が検査網から
// 抜ける状態を防ぐ。
const CONTAINER_SCOPED_PLUGINS = new Set(['remark-gfm']);
// 免除は無検査の別枠ではない: 「自身が emit する名前空間を直に指す」という分類根拠
// そのものを機械照合する。各エントリは自 namespace のセレクタ形 (selectorToken) と、
// theme ブリッジ (.dark 等) として自分の custom property だけを宣言するルールを許す
// 変数接頭辞 (ownVariablePrefix・宣言のみで描画に触れない) を持つ。
const EMITTED_NAMESPACE_SCOPED_PLUGINS = new Map<
  string,
  { selectorToken: RegExp; ownVariablePrefix?: string }
>([
  // BEM の要素 (__x)・修飾子 (--x) は自 namespace の続き。単一ハイフン継続
  // (.kfm-alert-like) は別クラスとして拒む
  ['remark-koyori-alerts', { selectorToken: /\.kfm-alert(?:__[\w-]+|--[\w-]+)?(?![\w-])/ }],
  [
    // starry-night 自身のルールは theme ブリッジ (.dark に --color-prettylights-syntax-*
    // を複写) のみ。@import する upstream シート (:root 変数固定) は vendored 上流であり
    // 本検査の対象外 (検査対象はサイドカー自身が書き足すルール)。
    'rehype-starry-night',
    { selectorToken: /\.pl-[\w-]/, ownVariablePrefix: '--color-prettylights-syntax-' },
  ],
]);
const CONTAINER_SCOPED_CSS_PATHS = SIDECAR_CSS_PATHS.filter((cssPath) =>
  CONTAINER_SCOPED_PLUGINS.has(path.basename(path.dirname(cssPath))),
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

/** ルール単位 ({selectors, body}) の列挙 (コメント・@import 文は除去。パーサの素朴さは上記と同じ) */
const extractRules = (css: string): Array<{ selectors: string[]; body: string }> =>
  css
    .replace(/\/\*[\s\S]*?\*\//g, '')
    .replace(/@import[^;]*;/g, '')
    .split('}')
    .map((block) => {
      const [selectorPart, body] = block.split('{');
      return {
        selectors: (selectorPart ?? '')
          .split(',')
          .map((selector) => selector.trim())
          .filter((selector) => selector.length > 0),
        body: (body ?? '').trim(),
      };
    })
    .filter((rule) => rule.selectors.length > 0);

/**
 * 免除サイドカーの自 namespace 逸脱ルールを列挙する。合格は二形のみ:
 * - 全セレクタが自身の emit 名前空間 (selectorToken) を指す
 * - ownVariablePrefix が定義され、本文が自変数の宣言だけの theme ブリッジ
 */
const emittedNamespaceViolations = (
  css: string,
  spec: { selectorToken: RegExp; ownVariablePrefix?: string },
): string[] =>
  extractRules(css)
    .filter((rule) => {
      if (rule.selectors.every((selector) => spec.selectorToken.test(selector))) return false;
      if (spec.ownVariablePrefix === undefined) return true;
      const declarations = rule.body
        .split(';')
        .map((declaration) => declaration.trim())
        .filter((declaration) => declaration.length > 0);
      return !(
        declarations.length > 0 &&
        declarations.every((declaration) => declaration.startsWith(spec.ownVariablePrefix ?? ''))
      );
    })
    .flatMap((rule) => rule.selectors);

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

describe('KFM サイドカー CSS の消費契約 (scope 一致の機構)', () => {
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

  it('remark-* / rehype-* の全サイドカーが scope 方式を明示分類される', () => {
    const discoveredPlugins = SIDECAR_CSS_PATHS.map((cssPath) =>
      path.basename(path.dirname(cssPath)),
    ).sort();
    const classifiedPlugins = [
      ...CONTAINER_SCOPED_PLUGINS,
      ...EMITTED_NAMESPACE_SCOPED_PLUGINS.keys(),
    ].sort();
    expect(discoveredPlugins).toEqual(classifiedPlugins);
  });

  it('検査器の陽性対照: 免除サイドカーの他人 namespace ルール・他変数ブリッジを拒む', () => {
    const spec = {
      selectorToken: /\.kfm-alert(?:__[\w-]+|--[\w-]+)?(?![\w-])/,
      ownVariablePrefix: '--kfm-alert-',
    };
    // 自 namespace を指すルール (BEM 要素・修飾子含む) と自変数だけのブリッジは通る
    expect(emittedNamespaceViolations('.kfm-alert { color: red }', spec)).toEqual([]);
    expect(emittedNamespaceViolations('.kfm-alert__title::before { color: red }', spec)).toEqual(
      [],
    );
    expect(emittedNamespaceViolations('.dark .kfm-alert--note { color: red }', spec)).toEqual([]);
    expect(emittedNamespaceViolations('.dark { --kfm-alert-bg: #000 }', spec)).toEqual([]);
    // 他人の要素を指す・部分一致・他変数や実プロパティ混じりのブリッジは逸脱
    expect(emittedNamespaceViolations('ul { margin: 0 }', spec)).toEqual(['ul']);
    expect(emittedNamespaceViolations('.kfm-alert-like { color: red }', spec)).toEqual([
      '.kfm-alert-like',
    ]);
    expect(emittedNamespaceViolations('.dark { --other-var: #000 }', spec)).toEqual(['.dark']);
    expect(emittedNamespaceViolations('.dark { --kfm-alert-bg: #000; color: red }', spec)).toEqual([
      '.dark',
    ]);
    // カンマ複数セレクタは全員が自 namespace でなければ逸脱 (巻き添え適用を防ぐ)
    expect(emittedNamespaceViolations('.kfm-alert, ul { color: red }', spec)).toEqual([
      '.kfm-alert',
      'ul',
    ]);
  });

  it('免除サイドカーの全ルールが自身の emit 名前空間限定 (theme ブリッジは自変数のみ)', () => {
    const discoveredPlugins = new Map(
      SIDECAR_CSS_PATHS.map((cssPath) => [path.basename(path.dirname(cssPath)), cssPath]),
    );
    for (const [plugin, spec] of EMITTED_NAMESPACE_SCOPED_PLUGINS) {
      const cssPath = discoveredPlugins.get(plugin);
      expect(cssPath, `${plugin} のサイドカー CSS が実在する`).toBeDefined();
      const violations = emittedNamespaceViolations(
        fs.readFileSync(cssPath as string, 'utf8'),
        spec,
      );
      expect(violations, `${plugin} の自 namespace 逸脱ルール`).toEqual([]);
    }
  });

  it('器 scope が必要なサイドカーの全ルールが器クラス子孫限定', () => {
    const discoveredPlugins = SIDECAR_CSS_PATHS.map((cssPath) =>
      path.basename(path.dirname(cssPath)),
    );
    for (const plugin of CONTAINER_SCOPED_PLUGINS) {
      expect(discoveredPlugins).toContain(plugin);
    }

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
});
