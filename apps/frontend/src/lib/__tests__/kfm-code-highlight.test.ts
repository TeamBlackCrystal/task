import { describe, expect, it } from 'vitest';
import { renderDescription } from '../markup-renderer';
import { starryNightSanitizeSchema } from '../rehype-starry-night';
import { gfmSanitizeSchema } from '../remark-gfm';

/**
 * style「属性」検出器。属性位置 (タグ開き内) の style= のみに反応する。
 * コードフェンス内の style= はテキストとしてエスケープされて出る (< は必ず実体参照化
 * される) ため、素の substring 検査では属性とテキストを区別できない — タグ開き内に
 * 限定することで「出力のどの要素にも style 属性が無い」を機械で主張する。
 */
const STYLE_ATTR_RE = /<[^>]*\sstyle\s*=/i;

describe('renderDescription (コードブロック着色)', () => {
  it('既知言語フェンスに pl- クラスが載る (language-* class も維持)', async () => {
    const html = await renderDescription('```ts\nconst x = 1;\n```');
    expect(html).toContain('language-ts');
    expect(html).toContain('<span class="pl-k">const</span>');
  });

  it('🔴 出力のどの要素にも style 属性が無い (FORBID_ATTR: [style] 契約の固定)', async () => {
    // 検出器の陽性対照: 属性位置の style は検出し、エスケープ済みテキストは誤検出しない
    expect('<p style="color:red">x</p>').toMatch(STYLE_ATTR_RE);
    expect('<code>&#x3C;div style="color:red">x</code>').not.toMatch(STYLE_ATTR_RE);

    // 属性として style を持ち込む生 HTML と、テキストとして style= を含むフェンスの両方を混ぜる
    const input = [
      '```ts\nconst s: string = `tpl`;\n```',
      '```html\n<div style="color:red">t</div>\n```',
      '```css\n.a { color: red; }\n```',
      '<span style="color:red">生HTML</span>',
    ].join('\n\n');
    const html = await renderDescription(input);
    expect(html).toContain('class="pl-'); // ハイライトが実際に効いた出力を見ている陽性対照
    expect(html).not.toMatch(STYLE_ATTR_RE);
  });

  it('未知言語フェンスは素のコードブロックへ落ち、内容はエスケープされたまま', async () => {
    const html = await renderDescription('```definitelynotalang\n<b>&\n```');
    // language-* class は残る (gfmSanitizeSchema の既存パターン) が、ハイライトはされない
    expect(html).toContain('language-definitelynotalang');
    expect(html).not.toContain('pl-');
    // 中身は要素化されず、エスケープ済みテキストのまま (<b> がタグとして出ない)
    expect(html).not.toContain('<b');
    expect(html).toMatch(/&(lt|#x3C);b/);
  });

  it('注入ペイロード (</code><script>…) がコードブロック外へ出ない', async () => {
    const html = await renderDescription('```ts\n</code><script>alert(1)</script>\n```');
    // まず「拒否でなく描画された」ことを主張する — 空出力は何も証明しない
    expect(html).toContain('<pre>');
    expect(html).toContain('script'); // ペイロードはハイライト済みテキストとして残っている
    // 封じ込め: script 要素は生まれず、code ブロックは 1 つのまま閉じている
    expect(html).not.toContain('<script');
    expect(html.match(/<code/g)).toHaveLength(1);
    expect(html.match(/<\/pre>/g)).toHaveLength(1);
  });

  it('ハイライト済み出力も class allowlist を通る (pl- と language-* が共存)', async () => {
    const html = await renderDescription('```rust\nfn main() {}\n```');
    expect(html).toContain('language-rust');
    expect(html).toContain('class="pl-');
    // 許可パターンは実装スキーマを単一ソースにする。テスト側で字種を複製すると、
    // language-c++ 等を足した際に実装でなく狭いテストの方が落ちるため。
    const classPatterns = [
      ...(gfmSanitizeSchema.classPatterns ?? []),
      ...(starryNightSanitizeSchema.classPatterns ?? []),
    ];
    for (const m of html.matchAll(/class="([^"]*)"/g)) {
      for (const token of m[1]!.split(/\s+/).filter(Boolean)) {
        expect(classPatterns.some((pattern) => pattern.test(token))).toBe(true);
      }
    }
  });
});
