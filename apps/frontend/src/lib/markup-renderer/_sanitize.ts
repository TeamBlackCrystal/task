/**
 * _sanitize.ts — DOMPurify 設定 (構造専任・registry 方式)。
 *
 * DOMPurify は HTML 構造の allowlist に専念し、CSS の意味解釈はしない:
 * - 🔴 FORBID_ATTR: ['style'] — inline style は一切通さない。プラグインは名前空間クラス
 *   でのみ装飾する契約であり、この FORBID を外すと kfm-sanitize テストが落ちる。
 * - class は既知トークン完全一致 allowlist (afterSanitizeAttributes フック)。DOMPurify は
 *   class の「値」を検査しないためフック必須。悪意 HTML がアプリ側クラス
 *   (modal-overlay 等) を騙る UI redressing を封じる。
 * - CUSTOM_ELEMENT_HANDLING は registry 登録制 (現在は kfm-mermaid を許可)。
 *   allowCustomizedBuiltInElements: false で is="" 経路を封鎖。
 *
 * 許可集合は各プラグインが export する SanitizeSchema を createRenderer({ sanitizeSchemas })
 * で合成した registry が単一ソース (コアはプラグインを import しない規約のまま、emit と
 * sanitize の許可が同時に増減する)。
 */
import DOMPurify from 'isomorphic-dompurify';

export type SanitizeSchema = {
  /** 許可するカスタム要素タグ (現在は kfm-mermaid。将来 kfm-animation 等が合流) */
  readonly tags?: readonly string[];
  /** カスタム要素タグごとの許可属性 (kfm-mermaid は属性なし) */
  readonly attrs?: Readonly<Record<string, readonly string[]>>;
  /** class 完全一致 allowlist トークン */
  readonly classTokens?: readonly string[];
  /** class パターン許可 (language-* 等の固定形パターンに限る) */
  readonly classPatterns?: readonly RegExp[];
};

type Registry = {
  readonly tags: ReadonlySet<string>;
  readonly attrsByTag: ReadonlyMap<string, ReadonlySet<string>>;
  readonly classTokens: ReadonlySet<string>;
  readonly classPatterns: readonly RegExp[];
};

export function buildRegistry(schemas: readonly SanitizeSchema[]): Registry {
  const tags = new Set<string>();
  const attrsByTag = new Map<string, Set<string>>();
  const classTokens = new Set<string>();
  const classPatterns: RegExp[] = [];
  for (const schema of schemas) {
    for (const tag of schema.tags ?? []) tags.add(tag);
    for (const [tag, attrs] of Object.entries(schema.attrs ?? {})) {
      const set = attrsByTag.get(tag) ?? new Set<string>();
      for (const attr of attrs) set.add(attr);
      attrsByTag.set(tag, set);
    }
    for (const token of schema.classTokens ?? []) classTokens.add(token);
    for (const pattern of schema.classPatterns ?? []) {
      // g / y フラグは lastIndex を跨いで保持し .test() が呼び出し履歴で真偽反転する
      // (同じ token が交互に許可/拒否される)。registry 組立時に fail-fast で弾く。
      if (pattern.global || pattern.sticky) {
        throw new Error(
          `[markup-renderer] classPatterns の正規表現 ${String(pattern)} に g / y フラグは使えない ` +
            '(lastIndex 状態で .test() の判定が反転する)',
        );
      }
      classPatterns.push(pattern);
    }
  }
  return { tags, attrsByTag, classTokens, classPatterns };
}

// isomorphic-dompurify の DOMPurify はモジュール singleton で addHook もグローバルに効く。
// 常駐フックは素の DOMPurify.sanitize を使う無関係コードの出力まで書き換えてしまう
// (registry 不在の fail-closed で class 全消去) ため、各 sanitize() 呼び出しの直前に
// addHook し、呼び出し後は finally で必ず removeHook する。
// - DOMPurify.sanitize は同期であり、据え付け〜撤去の間に他者のコードは走らない。
// - removeHook は関数指定の除去 (dompurify 3.x) を使い、他者が据えたフックには触れない。
// - 別インスタンス化 (createDOMPurify(window) 相当) は SSR 側 window (jsdom) が
//   isomorphic-dompurify 内部に閉じており、dompurify / jsdom の直接依存追加なしには
//   成立しないためこの方式を採る。
function isAllowedClassToken(token: string, registry: Registry): boolean {
  if (registry.classTokens.has(token)) return true;
  return registry.classPatterns.some((pattern) => pattern.test(token));
}

function createClassAllowlistHook(registry: Registry): (node: Node) => void {
  return (node) => {
    const element = node as Element;
    if (typeof element.hasAttribute !== 'function') return;
    if (!element.hasAttribute('class')) return;
    const kept = (element.getAttribute('class') ?? '')
      .split(/[ \t\n\r\f]+/)
      .filter((token) => token.length > 0 && isAllowedClassToken(token, registry));
    if (kept.length > 0) {
      element.setAttribute('class', kept.join(' '));
    } else {
      element.removeAttribute('class');
    }
  };
}

/** schemas を合成した registry で閉じた sanitizer を返す */
export function createSanitizer(schemas: readonly SanitizeSchema[]): (html: string) => string {
  const registry = buildRegistry(schemas);
  const hook = createClassAllowlistHook(registry);
  const config = {
    // GFM 出力は HTML のみ (SVG / MathML は不要ゆえ落とす)
    USE_PROFILES: { html: true },
    FORBID_ATTR: ['style'],
    CUSTOM_ELEMENT_HANDLING: {
      tagNameCheck: (tagName: string) => registry.tags.has(tagName),
      attributeNameCheck: (attrName: string, tagName?: string) =>
        tagName !== undefined && (registry.attrsByTag.get(tagName)?.has(attrName) ?? false),
      allowCustomizedBuiltInElements: false,
    },
  };
  return (html: string): string => {
    DOMPurify.addHook('afterSanitizeAttributes', hook);
    try {
      return DOMPurify.sanitize(html, config);
    } finally {
      DOMPurify.removeHook('afterSanitizeAttributes', hook);
    }
  };
}
