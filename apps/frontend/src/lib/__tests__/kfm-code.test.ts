import { afterEach, beforeAll, describe, expect, it, vi } from 'vitest';
import { createRenderer, renderDescription } from '../markup-renderer';
import { createSanitizer } from '../markup-renderer/_sanitize';
import { createRehypeStarryNight, starryNightSanitizeSchema } from '../rehype-starry-night';
import {
  createKfmCodeElement,
  KFM_CODE_COPY_CLASS,
  KFM_CODE_TAG,
} from '../rehype-kfm-code/element';
import { kfmCodeSanitizeSchema, rehypeKfmCode } from '../rehype-kfm-code';
import { gfmSanitizeSchema, remarkGfm } from '../remark-gfm';

/**
 * rehype-kfm-code の二層を検査する (kfm-mermaid の試験の流儀):
 * - rehype 層 (SSR): pre>code を <kfm-code data-lang data-title> で包み、行を
 *   <span class="pl-line"> で切る。sanitize (DOMPurify) を通った後の姿 =
 *   renderDescription の実出力で見る。
 * - client 層: custom element が connectedCallback でコピーボタンを足し、
 *   clipboard へのコピーと表示切替 (data-kfm-code-copy) を担う。
 */

describe('rehype-kfm-code (SSR / rehype 層)', () => {
  it('言語付きフェンスは <kfm-code data-lang> に包まれ、pre>code と着色は保たれる', async () => {
    const html = await renderDescription('```ts\nconst x = 1;\n```');
    expect(html).toContain('<kfm-code data-lang="ts">');
    expect(html).toContain('</kfm-code>');
    expect(html).toContain('<pre>');
    expect(html).toContain('language-ts');
    expect(html).toContain('pl-k'); // 着色 (starry-night) が行分割後も効いている陽性対照
  });

  it('言語無しフェンスも包む (data-lang 無し)', async () => {
    const html = await renderDescription('```\nplain text\n```');
    expect(html).toContain('<kfm-code>');
    expect(html).not.toContain('data-lang');
  });

  it('inline code は包まない (対象は pre>code のみ)', async () => {
    const html = await renderDescription('本文 `inline` です');
    expect(html).not.toContain('kfm-code');
    expect(html).toContain('<code>inline</code>');
  });

  it('SSR HTML にコピーボタンは入らない (button は client の custom element が足す)', async () => {
    const html = await renderDescription('```ts\nconst x = 1;\n```');
    expect(html).not.toContain('button');
    expect(html).not.toContain(KFM_CODE_COPY_CLASS);
    // コピー状態も client が立てる値。SSR 出力に現れない
    expect(html).not.toContain('data-kfm-code-copy');
  });

  describe('情報文字列の解釈 (data-title)', () => {
    it('```ts title="src/foo.ts" は data-lang="ts" と data-title="src/foo.ts"', async () => {
      const html = await renderDescription('```ts title="src/foo.ts"\nconst x = 1;\n```');
      expect(html).toContain('<kfm-code data-lang="ts" data-title="src/foo.ts">');
    });

    it('```ts:src/foo.ts も data-lang="ts" と data-title="src/foo.ts"', async () => {
      const html = await renderDescription('```ts:src/foo.ts\nconst x = 1;\n```');
      expect(html).toContain('<kfm-code data-lang="ts" data-title="src/foo.ts">');
      // `:` 形式の language-ts:src/foo.ts class は sanitize の許可字種の外で残らない
      expect(html).not.toContain('language-ts:');
    });

    it('両方あれば title="…" (明示) が勝つ', async () => {
      const html = await renderDescription('```ts:one.ts title="two.ts"\nconst x = 1;\n```');
      expect(html).toContain('data-title="two.ts"');
      expect(html).not.toContain('one.ts');
    });

    it('どちらも無ければ data-title は出ない', async () => {
      const html = await renderDescription('```ts\nconst x = 1;\n```');
      expect(html).not.toContain('data-title');
    });

    it('title の値は属性としてエスケープされ、要素として漏れない', async () => {
      const html = await renderDescription('```ts title="src/<bar> & foo.ts"\nconst x = 1;\n```');
      // DOMPurify の直列化は属性値の < を残す (引用属性内の < は合法)。よって
      // 「要素として漏れない」は属性値を除いた残りに <bar が無いことで主張し、
      // エスケープ表記の細部 (&#x3C; か < か) は断定しない
      const withoutAttributeValues = html.replace(/"[^"]*"/g, '""');
      expect(withoutAttributeValues).not.toContain('<bar');
      expect(html).toMatch(/data-title="[^"]*bar[^"]*foo\.ts"/);
    });

    it('情報文字列の他の語 (title= 以外) は無視する', async () => {
      const html = await renderDescription('```ts showLineNumbers highlight\nconst x = 1;\n```');
      expect(html).toContain('<kfm-code data-lang="ts">');
      expect(html).not.toContain('showLineNumbers');
    });
  });

  describe('行分割 (pl-line)', () => {
    it('行ごとに <span class="pl-line"> で切られ、末尾の改行で空行を余分に出さない', async () => {
      const html = await renderDescription('```ts\nconst x = 1;\nconst y = 2;\n```');
      expect(html.match(/pl-line/g)).toHaveLength(2);
    });

    it('中間の空行は 1 行として残る', async () => {
      const html = await renderDescription('```ts\nconst x = 1;\n\nconst y = 2;\n```');
      expect(html.match(/pl-line/g)).toHaveLength(3);
    });

    it('行を跨ぐ着色 span (ブロックコメント) は行ごとに分かれ、両行で着色が保たれる', async () => {
      const html = await renderDescription('```ts\n/* 一行目\n二行目 */\n```');
      expect(html.match(/pl-line/g)).toHaveLength(2);
      // starry-night は複数行コメントを 1 つの pl-c span で出す。分割後も両方の行に
      // pl-c が居る (片方が素テキストへ落ちていたら跨ぎ分割が壊れている)
      expect(html.match(/pl-c/g)?.length ?? 0).toBeGreaterThanOrEqual(2);
      expect(html).toContain('一行目');
      expect(html).toContain('二行目');
    });

    it('タグを剥いだ本文は改行込みで原文どおり (コピーと CR 正規化の土台)', async () => {
      const html = await renderDescription('```ts\nconst x = 1;\nconst y = 2;\n```');
      const text = html.replace(/<[^>]+>/g, '');
      expect(text).toContain('const x = 1;\nconst y = 2;\n');
    });
  });

  it('kfm-mermaid のフェンスは包まない (remark 層で置換済み)', async () => {
    const html = await renderDescription('```mermaid\nflowchart TD\n  A --> B\n```');
    expect(html).toContain('<kfm-mermaid>');
    expect(html).not.toContain('kfm-code');
    expect(html).not.toContain('<pre');
  });

  it('markdown 直書きの生 <kfm-code> (属性付き) は不活性化される (フェンス経由のみが正規経路)', async () => {
    const html = await renderDescription('<kfm-code data-lang="ts" onclick="pwn()">hi</kfm-code>');
    expect(html).not.toContain('<kfm-code');
    expect(html).not.toContain('onclick');
  });

  it('SSR/CSR 同一性: 同一入力は独立 renderer 間で同一 HTML、L1 cache 越しも同一', async () => {
    const input = '```ts title="src/foo.ts"\nconst x = 1;\n```';
    const makeRenderer = () =>
      createRenderer({
        profiles: {
          github: {
            remarkPlugins: [remarkGfm],
            rehypePlugins: [createRehypeStarryNight(), rehypeKfmCode],
          },
        },
        sanitizeSchemas: [gfmSanitizeSchema, starryNightSanitizeSchema, kfmCodeSanitizeSchema],
      });
    const first = makeRenderer();
    const second = makeRenderer();
    const initial = await first(input);
    expect(initial).toContain('<kfm-code data-lang="ts" data-title="src/foo.ts">');
    // L1 cache 越し (同一 renderer 2 回目はキャッシュから返る)
    expect(await first(input)).toBe(initial);
    // 独立 renderer (CSR 相当) でも同一 HTML
    expect(await second(input)).toBe(initial);
  });
});

describe('kfmCodeSanitizeSchema (sanitize の許可と拒否)', () => {
  const sanitize = createSanitizer([kfmCodeSanitizeSchema]);

  it('kfm-code と data-lang / data-title と pl-line は通る', () => {
    const html = sanitize(
      '<kfm-code data-lang="ts" data-title="src/foo.ts"><pre><code><span class="pl-line">x\n</span></code></pre></kfm-code>',
    );
    expect(html).toContain('<kfm-code data-lang="ts" data-title="src/foo.ts">');
    expect(html).toContain('class="pl-line"');
  });

  it('宣言外の属性 (onclick / 独自属性) は通らない (要素は残る陽性対照)', () => {
    const html = sanitize('<kfm-code onclick="alert(1)" unlisted="x">子</kfm-code>');
    expect(html).toContain('<kfm-code');
    expect(html).not.toContain('onclick');
    expect(html).not.toContain('unlisted');
  });

  it('pl-line 以外の未許可 class は落ちる (pl-line は残る陽性対照)', () => {
    const html = sanitize('<span class="pl-line modal-overlay">x</span>');
    expect(html).toContain('pl-line');
    expect(html).not.toContain('modal-overlay');
  });

  it('本番 registry (renderDescription 実出力) でも kfm-code 一式が通る', async () => {
    const html = await renderDescription('```ts title="a.ts"\nconst x = 1;\n```');
    expect(html).toContain('<kfm-code data-lang="ts" data-title="a.ts">');
    expect(html).toContain('class="pl-line"');
  });
});

describe('KfmCodeElement (client 層・コピーボタン)', () => {
  beforeAll(() => {
    if (customElements.get(KFM_CODE_TAG) === undefined) {
      customElements.define(KFM_CODE_TAG, createKfmCodeElement());
    }
  });

  afterEach(() => {
    document.body.innerHTML = '';
    // 各試験が据えた clipboard stub を撤去する (jsdom 既定は clipboard 不在)
    delete (navigator as { clipboard?: unknown }).clipboard;
    vi.useRealTimers();
  });

  function stubClipboard(writeText: (text: string) => Promise<void>): void {
    Object.defineProperty(navigator, 'clipboard', {
      value: { writeText },
      configurable: true,
    });
  }

  /** SSR 出力相当の light DOM (行 span は行末改行を中に持つ) で接続する */
  function mount(lines: readonly string[]): HTMLElement {
    const element = document.createElement(KFM_CODE_TAG);
    const spans = lines.map((line) => `<span class="pl-line">${line}\n</span>`).join('');
    element.innerHTML = `<pre><code>${spans}</code></pre>`;
    document.body.append(element);
    return element;
  }

  function buttonOf(element: HTMLElement): HTMLButtonElement {
    const button = element.querySelector<HTMLButtonElement>(`.${KFM_CODE_COPY_CLASS}`);
    if (button === null) throw new Error('コピーボタンが無い');
    return button;
  }

  /**
   * 表示中アイコンの識別。lucide の実 path (element.ts の ICON_NODES と同値) で見る:
   * copy だけが rect を持ち、check と x は path の d 値で見分ける。
   */
  function iconKindOf(button: HTMLButtonElement): 'copy' | 'check' | 'x' | 'unknown' {
    const svg = button.querySelector('svg');
    if (svg === null) return 'unknown';
    if (svg.querySelector('rect') !== null) return 'copy';
    if (svg.querySelector('path[d="M20 6 9 17l-5-5"]') !== null) return 'check';
    if (svg.querySelector('path[d="M18 6 6 18"]') !== null) return 'x';
    return 'unknown';
  }

  it('接続でコピーボタン (copy アイコン・aria-label「コピー」) が light DOM へ入り、再接続でも二重には入らない', () => {
    const element = mount(['const x = 1;']);
    expect(element.querySelectorAll(`.${KFM_CODE_COPY_CLASS}`)).toHaveLength(1);
    const button = buttonOf(element);
    expect(button.type).toBe('button');
    expect(button.getAttribute('aria-label')).toBe('コピー');
    expect(button.title).toBe('コピー');
    // 意味は aria-label が担い、図は読み上げから隠す
    expect(button.querySelector('svg')?.getAttribute('aria-hidden')).toBe('true');
    expect(iconKindOf(button)).toBe('copy');
    element.remove();
    document.body.append(element);
    expect(element.querySelectorAll(`.${KFM_CODE_COPY_CLASS}`)).toHaveLength(1);
  });

  it('押すと code の textContent (行番号を含まぬ本文) が clipboard へ写り、check 表示「コピーしました」に変わる', async () => {
    const writeText = vi.fn(async () => undefined);
    stubClipboard(writeText);
    const element = mount(['const x = 1;', 'const y = 2;']);
    buttonOf(element).click();
    await vi.waitFor(() => expect(element.dataset.kfmCodeCopy).toBe('copied'));
    // 行番号は CSS counter (::before) 描画で DOM テキストに無いため、本文だけが写る
    expect(writeText).toHaveBeenCalledWith('const x = 1;\nconst y = 2;\n');
    expect(buttonOf(element).getAttribute('aria-label')).toBe('コピーしました');
    expect(buttonOf(element).title).toBe('コピーしました');
    expect(iconKindOf(buttonOf(element))).toBe('check');
  });

  it('約 2 秒で copy 表示「コピー」へ戻り、状態属性も消える', async () => {
    vi.useFakeTimers();
    stubClipboard(vi.fn(async () => undefined));
    const element = mount(['const x = 1;']);
    buttonOf(element).click();
    await vi.advanceTimersByTimeAsync(0); // click ハンドラ内の await を流す
    expect(element.dataset.kfmCodeCopy).toBe('copied');
    expect(iconKindOf(buttonOf(element))).toBe('check');
    await vi.advanceTimersByTimeAsync(2000);
    expect(element.dataset.kfmCodeCopy).toBeUndefined();
    expect(buttonOf(element).getAttribute('aria-label')).toBe('コピー');
    expect(iconKindOf(buttonOf(element))).toBe('copy');
  });

  it('clipboard が無い環境では失敗を握り潰さず x 表示「コピーできませんでした」で示す', async () => {
    const consoleError = vi.spyOn(console, 'error').mockImplementation(() => undefined);
    // jsdom は navigator.clipboard を実装している (実測) ため、「不在」は明示 stub で作る
    Object.defineProperty(navigator, 'clipboard', { value: undefined, configurable: true });
    const element = mount(['const x = 1;']);
    buttonOf(element).click();
    await vi.waitFor(() => expect(element.dataset.kfmCodeCopy).toBe('failed'));
    expect(buttonOf(element).getAttribute('aria-label')).toBe('コピーできませんでした');
    expect(iconKindOf(buttonOf(element))).toBe('x');
    expect(consoleError).toHaveBeenCalledWith('[kfm-code] copy failed', expect.any(Error));
    consoleError.mockRestore();
  });

  it('writeText の reject も x 表示へ倒す', async () => {
    const consoleError = vi.spyOn(console, 'error').mockImplementation(() => undefined);
    stubClipboard(
      vi.fn(async () => {
        throw new Error('denied');
      }),
    );
    const element = mount(['const x = 1;']);
    buttonOf(element).click();
    await vi.waitFor(() => expect(element.dataset.kfmCodeCopy).toBe('failed'));
    expect(iconKindOf(buttonOf(element))).toBe('x');
    expect(consoleError).toHaveBeenCalledWith(
      '[kfm-code] copy failed',
      expect.objectContaining({ message: 'denied' }),
    );
    consoleError.mockRestore();
  });

  it('コピー成功表示中に remove → append しても再接続後は初期状態のまま (タイマー進行でも変わらぬ)', async () => {
    vi.useFakeTimers();
    stubClipboard(vi.fn(async () => undefined));
    const element = mount(['const x = 1;']);
    buttonOf(element).click();
    await vi.advanceTimersByTimeAsync(0);
    expect(element.dataset.kfmCodeCopy).toBe('copied');
    expect(iconKindOf(buttonOf(element))).toBe('check');
    expect(buttonOf(element).getAttribute('aria-label')).toBe('コピーしました');

    element.remove();
    document.body.append(element);

    expect(element.dataset.kfmCodeCopy).toBeUndefined();
    expect(iconKindOf(buttonOf(element))).toBe('copy');
    expect(buttonOf(element).getAttribute('aria-label')).toBe('コピー');
    expect(buttonOf(element).title).toBe('コピー');

    await vi.advanceTimersByTimeAsync(2500);
    expect(element.dataset.kfmCodeCopy).toBeUndefined();
    expect(iconKindOf(buttonOf(element))).toBe('copy');
    expect(buttonOf(element).getAttribute('aria-label')).toBe('コピー');
  });

  it('コピー失敗表示中に remove → append しても再接続後は初期状態のまま (タイマー進行でも変わらぬ)', async () => {
    vi.useFakeTimers();
    const consoleError = vi.spyOn(console, 'error').mockImplementation(() => undefined);
    stubClipboard(
      vi.fn(async () => {
        throw new Error('denied');
      }),
    );
    const element = mount(['const x = 1;']);
    buttonOf(element).click();
    await vi.advanceTimersByTimeAsync(0);
    expect(element.dataset.kfmCodeCopy).toBe('failed');
    expect(iconKindOf(buttonOf(element))).toBe('x');
    expect(buttonOf(element).getAttribute('aria-label')).toBe('コピーできませんでした');

    element.remove();
    document.body.append(element);

    expect(element.dataset.kfmCodeCopy).toBeUndefined();
    expect(iconKindOf(buttonOf(element))).toBe('copy');
    expect(buttonOf(element).getAttribute('aria-label')).toBe('コピー');
    expect(buttonOf(element).title).toBe('コピー');

    await vi.advanceTimersByTimeAsync(2500);
    expect(element.dataset.kfmCodeCopy).toBeUndefined();
    expect(iconKindOf(buttonOf(element))).toBe('copy');
    expect(buttonOf(element).getAttribute('aria-label')).toBe('コピー');
    consoleError.mockRestore();
  });
});
