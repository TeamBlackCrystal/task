import DOMPurify from 'isomorphic-dompurify';
import { describe, expect, it } from 'vitest';
import { buildRegistry, createSanitizer } from '../markup-renderer/_sanitize';
import { gfmSanitizeSchema } from '../remark-gfm';
import { koyoriAlertsSanitizeSchema } from '../remark-koyori-alerts';

// mermaid を含まない sanitize registry の単体検査用構成
const sanitize = createSanitizer([gfmSanitizeSchema, koyoriAlertsSanitizeSchema]);

describe('createSanitizer (🔴 FORBID_ATTR: style)', () => {
  it('inline style 入力は style を落として出る (内容と許可 class は残る陽性対照)', () => {
    const html = sanitize('<p style="color:red;background:url(//evil)" class="kfm-alert">本文</p>');
    expect(html).not.toContain('style');
    expect(html).toContain('本文');
    expect(html).toContain('class="kfm-alert"');
  });

  it('どのタグでも style 属性は残らない', () => {
    const html = sanitize('<div style="position:fixed"><span style="display:none">x</span></div>');
    expect(html).not.toContain('style=');
    expect(html).toContain('x');
  });
});

describe('createSanitizer (class 完全一致 allowlist)', () => {
  it('アプリ側クラスの騙り (modal-overlay 等) を剥がし、許可トークンだけ残す', () => {
    const html = sanitize('<div class="modal-overlay kfm-alert kfm-alert--note">x</div>');
    expect(html).not.toContain('modal-overlay');
    expect(html).toContain('kfm-alert');
    expect(html).toContain('kfm-alert--note');
  });

  it('許可トークンが 1 つも無ければ class 属性ごと除去する', () => {
    const html = sanitize('<div class="evil-class another">x</div>');
    expect(html).not.toContain('class=');
    expect(html).toContain('x');
  });

  it('language-* パターンは実在言語表記の字種 ([A-Za-z0-9+#._-]) を許可する', () => {
    // 大文字・+・# を含む実在表記 (yupix 実測 5 例は kfm-renderer テスト側で固定)
    expect(sanitize('<code class="language-ts">x</code>')).toContain('language-ts');
    expect(sanitize('<code class="language-TS">x</code>')).toContain('language-TS');
    expect(sanitize('<code class="language-C++">x</code>')).toContain('language-C++');
    expect(sanitize('<code class="language-c#">x</code>')).toContain('language-c#');
    // 許可字種の外は依然弾く (空 suffix・スペース・引用符等の構造汚染)
    expect(sanitize('<code class="language-">x</code>')).not.toContain('class=');
    expect(sanitize('<code class="language-a&quot;b">x</code>')).not.toContain('class=');
  });

  it('前方一致・部分一致では通らない (完全一致のみ)', () => {
    const html = sanitize('<div class="kfm-alert-evil kfm-alert__title2">x</div>');
    expect(html).not.toContain('kfm-alert-evil');
    expect(html).not.toContain('kfm-alert__title2');
  });
});

describe('buildRegistry (classPatterns の g / y フラグ拒否)', () => {
  // g / y フラグの正規表現は lastIndex を保持し、.test() が呼び出し履歴で真偽反転する
  // (同じ token が交互に許可/拒否)。判定が非決定になるため registry 組立時に fail-fast。
  it.each([/^language-x$/g, /^language-x$/y, /^language-x$/gy])(
    'フラグ付き %s は registry 組立時に throw',
    (pattern) => {
      expect(() => buildRegistry([{ classPatterns: [pattern] }])).toThrow('フラグ');
      expect(() => createSanitizer([{ classPatterns: [pattern] }])).toThrow('フラグ');
    },
  );

  it('フラグ無し (および i 等の無害フラグ) は通る陽性対照', () => {
    expect(() =>
      buildRegistry([{ classPatterns: [/^language-x$/, /^language-y$/i] }]),
    ).not.toThrow();
  });
});

describe('createSanitizer (URI 契約)', () => {
  it('data: 画像 URI は img src で通る (DOMPurify 既定・仕様書に明記)', () => {
    const html = sanitize('<img src="data:image/png;base64,iVBORw0KGgo=" alt="x">');
    expect(html).toContain('<img');
    expect(html).toContain('src="data:image/png;base64,iVBORw0KGgo="');
  });

  it('data: URI は a href では通らない (data: 許可は画像系タグに限る)', () => {
    const html = sanitize('<a href="data:text/html;base64,PHNjcmlwdD4=">リンク</a>');
    expect(html).not.toContain('data:');
    expect(html).toContain('リンク');
  });
});

describe('createSanitizer (XSS 基本)', () => {
  it('script タグを除去する', () => {
    const html = sanitize('前<script>alert(1)</script>後');
    expect(html).not.toContain('<script');
    expect(html).toContain('前');
    expect(html).toContain('後');
  });

  it('img onerror を除去する (img 自体は残る陽性対照)', () => {
    const html = sanitize('<img src="x.png" onerror="alert(1)">');
    expect(html).not.toContain('onerror');
    expect(html).toContain('<img');
    expect(html).toContain('src="x.png"');
  });

  it('javascript: URL を除去する', () => {
    const html = sanitize('<a href="javascript:alert(1)">リンク</a>');
    expect(html).not.toContain('javascript:');
    expect(html).toContain('リンク');
  });
});

describe('createSanitizer (フックは sanitize() 呼び出しごとに据え付けて撤去する)', () => {
  it('sanitizer 構築後も素の DOMPurify.sanitize は class を保持する (グローバル汚染なし)', () => {
    // 本番構成 sanitizer (module top の createSanitizer) が構築済みの状態で、レンダラを
    // 経由しない素の DOMPurify 利用者が影響を受けないこと。常駐フックが残っていると
    // registry 不在の fail-closed で無関係な HTML の class が全消去され、ここが落ちる。
    const html = DOMPurify.sanitize('<div class="card">x</div>');
    expect(html).toContain('class="card"');
    expect(html).toContain('x');
  });

  it('sanitizer 実行後もフックは残留しない (素の DOMPurify.sanitize は class を保持)', () => {
    sanitize('<p class="kfm-alert">本文</p>'); // フックの据え付け→撤去を一巡させる
    const html = DOMPurify.sanitize('<div class="card">y</div>');
    expect(html).toContain('class="card"');
  });

  it('sanitizer 経由では class allowlist が依然効く (スコープ化で検査が弱まらない陽性対照)', () => {
    const html = sanitize('<div class="card kfm-alert">x</div>');
    expect(html).not.toContain('card');
    expect(html).toContain('kfm-alert');
  });
});

describe('createSanitizer (カスタム要素 registry)', () => {
  it('未登録カスタム要素タグは除去する (kfm-animation は未登録)', () => {
    const html = sanitize('<kfm-animation fn="spin" speed="3s">子テキスト</kfm-animation>');
    expect(html).not.toContain('kfm-animation');
    expect(html).toContain('子テキスト');
  });

  it('is="" 経路 (customized built-in) を封鎖する (DOMPurify は is の値を空にする)', () => {
    const html = sanitize('<div is="evil-widget">x</div>');
    expect(html).not.toContain('evil-widget');
    expect(html).toContain('x');
  });

  it('registry に登録すればタグと宣言済み属性のみ通る (registry 機構の陽性対照)', () => {
    const withElement = createSanitizer([
      { tags: ['kfm-test-element'], attrs: { 'kfm-test-element': ['speed'] } },
    ]);
    const html = withElement(
      '<kfm-test-element speed="3s" onclick="alert(1)" unlisted="x">子</kfm-test-element>',
    );
    expect(html).toContain('<kfm-test-element');
    expect(html).toContain('speed="3s"');
    expect(html).not.toContain('onclick');
    expect(html).not.toContain('unlisted');
  });

  it('registry の違いは sanitizer 間で漏れない (排他制御)', () => {
    const withElement = createSanitizer([{ tags: ['kfm-test-element'] }]);
    expect(withElement('<kfm-test-element>x</kfm-test-element>')).toContain('kfm-test-element');
    // Phase 1 構成の sanitize では同じ入力が通らない
    expect(sanitize('<kfm-test-element>x</kfm-test-element>')).not.toContain('kfm-test-element');
  });
});
