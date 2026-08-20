import { afterEach, beforeAll, describe, expect, it, vi } from 'vitest';
import { renderDescription } from '../markup-renderer';
import { createKfmMermaidElement, KFM_MERMAID_TAG } from '../remark-kfm-mermaid/element';

/**
 * kfm-mermaid の二層を検査する:
 * - remark 層 (SSR): mermaid フェンス → 不活性 <kfm-mermaid> ＋エスケープ済みソース。
 *   sanitize (DOMPurify) を通った後の姿 = renderDescription の実出力で見る。
 * - client 層: custom element が connectedCallback で mermaid を遅延 import し、
 *   shadow DOM へ SVG を描き、完了/失敗を data-kfm-mermaid 状態で表す。
 *   mermaid 本体は重量ゆえ mock し、契約 (呼び出し形・状態遷移) を固定する。
 */

const mermaidMock = vi.hoisted(() => {
  return {
    /** vi.mock の factory が走った (= mermaid が import された) ことの観測点 */
    imported: false,
    initialize: vi.fn(),
    render: vi.fn(async (_id: string, _source: string) => ({
      svg: '<svg><g class="kfm-mermaid-test-probe"></g></svg>',
    })),
  };
});

vi.mock('mermaid', () => {
  mermaidMock.imported = true;
  return {
    default: { initialize: mermaidMock.initialize, render: mermaidMock.render },
  };
});

describe('remark-kfm-mermaid (SSR / remark 層)', () => {
  it('mermaid フェンスは <kfm-mermaid> ＋ light DOM ソーステキストになる (pre>code は出ない)', async () => {
    const html = await renderDescription('```mermaid\nflowchart TD\n  A --> B\n```');
    expect(html).toContain('<kfm-mermaid>');
    expect(html).toContain('flowchart TD');
    // 既定 handler の pre>code へ落ちていない ＝ rehype 層 (着色) はこのノードを見ない
    expect(html).not.toContain('<pre');
    expect(html).not.toContain('<code');
    // SVG をサーバで焼いていない
    expect(html).not.toContain('<svg');
    // 状態属性はレンダリング結果として client が立てるもの。SSR 出力に現れない
    expect(html).not.toContain('data-kfm-mermaid');
  });

  it('言語名は case-insensitive (```Mermaid も図として扱う)', async () => {
    const html = await renderDescription('```Mermaid\nflowchart TD\n  A --> B\n```');
    expect(html).toContain('<kfm-mermaid>');
  });

  it('ソース内のタグ表記はエスケープされたテキストのまま残る', async () => {
    const html = await renderDescription('```mermaid\nflowchart TD\n  A["<b>太字</b>"] --> B\n```');
    // 生タグとしては現れず (エスケープ表記の細部は断定しない)、内容は保たれる
    expect(html).not.toContain('<b>');
    expect(html).toContain('太字');
  });

  it('mermaid 以外のフェンスは無傷 (陽性対照: pre>code へ落ちる)', async () => {
    const html = await renderDescription('```ts\nconst untouched = true;\n```');
    expect(html).not.toContain('kfm-mermaid');
    expect(html).toContain('<pre');
  });

  it('markdown 直書きの生 <kfm-mermaid> (属性付き) は不活性化される (フェンス経由のみが正規経路)', async () => {
    const html = await renderDescription('<kfm-mermaid onclick="pwn()">hi</kfm-mermaid>');
    expect(html).not.toContain('<kfm-mermaid');
    expect(html).not.toContain('onclick');
  });
});

describe('KfmMermaidElement (client 層・mermaid は mock)', () => {
  beforeAll(() => {
    if (customElements.get(KFM_MERMAID_TAG) === undefined) {
      customElements.define(KFM_MERMAID_TAG, createKfmMermaidElement());
    }
  });

  afterEach(() => {
    document.body.innerHTML = '';
    mermaidMock.initialize.mockClear();
    mermaidMock.render.mockClear();
    mermaidMock.render.mockImplementation(async () => ({
      svg: '<svg><g class="kfm-mermaid-test-probe"></g></svg>',
    }));
  });

  /** 接続して状態が立つまで待つ (時間待ちではなく data-kfm-mermaid の状態待ち) */
  async function mount(source: string, options: { dark?: boolean } = {}): Promise<HTMLElement> {
    const container = document.createElement('div');
    if (options.dark === true) container.className = 'dark';
    const element = document.createElement(KFM_MERMAID_TAG);
    element.textContent = source;
    container.append(element);
    document.body.append(container);
    await vi.waitFor(() => {
      if (element.dataset.kfmMermaid === undefined) throw new Error('まだ描画中');
    });
    return element;
  }

  it('モジュール評価だけでは mermaid を import しない (dynamic import 契約。接続時に初めて落ちてくる)', () => {
    // 本テストはファイル内の最初の要素テストとして走る (mount より前)
    expect(mermaidMock.imported).toBe(false);
  });

  it('接続で描画し data-kfm-mermaid="rendered" が立ち、shadow DOM に SVG が入る', async () => {
    const source = 'flowchart TD\n  A --> B';
    const element = await mount(source);
    expect(mermaidMock.imported).toBe(true);
    expect(element.dataset.kfmMermaid).toBe('rendered');
    expect(element.shadowRoot).not.toBeNull();
    expect(element.shadowRoot?.querySelector('svg')).not.toBeNull();
    // 描画契約: ソースは light DOM の textContent がそのまま渡る
    expect(mermaidMock.render).toHaveBeenCalledWith(
      expect.stringContaining('kfm-mermaid-'),
      source,
    );
  });

  it('securityLevel strict / startOnLoad false / suppressErrorRendering を固定で渡す', async () => {
    await mount('flowchart TD\n  A --> B');
    expect(mermaidMock.initialize).toHaveBeenCalledWith(
      expect.objectContaining({
        securityLevel: 'strict',
        startOnLoad: false,
        suppressErrorRendering: true,
      }),
    );
  });

  it('テーマは .dark ancestor で切り替わる (light=default / dark=dark)', async () => {
    await mount('flowchart TD\n  A --> B');
    expect(mermaidMock.initialize).toHaveBeenLastCalledWith(
      expect.objectContaining({ theme: 'default' }),
    );
    await mount('flowchart TD\n  A --> B', { dark: true });
    expect(mermaidMock.initialize).toHaveBeenLastCalledWith(
      expect.objectContaining({ theme: 'dark' }),
    );
  });

  it('描画失敗は data-kfm-mermaid="error" へ倒れ、shadow を張らずソーステキストが残る', async () => {
    const consoleError = vi.spyOn(console, 'error').mockImplementation(() => undefined);
    mermaidMock.render.mockImplementation(async () => {
      throw new Error('Parse error');
    });
    const source = 'flowchart TD\n  A[閉じ括弧がない --> B';
    const element = await mount(source);
    expect(element.dataset.kfmMermaid).toBe('error');
    // 失敗時に shadow を張るとフォールバック (light DOM ソース) まで隠れるため張らない
    expect(element.shadowRoot).toBeNull();
    expect(element.textContent).toBe(source);
    expect(consoleError).toHaveBeenCalledWith(
      '[kfm-mermaid] render failed',
      expect.objectContaining({ message: 'Parse error' }),
    );
    consoleError.mockRestore();
  });

  it('XML 非適格でも HTML として安全な SVG (xlink prefix 未宣言) は rendered (検査は sink と同じ HTML パース)', async () => {
    // 実測: click 付き flowchart で mermaid は <a xlink:href=…> を xmlns:xlink 宣言なしで
    // 出力する。XML パーサはこれを parsererror にするが、sink (shadow への HTML パース)
    // では有効。検査を XML パースへ戻すとこのテストが落ちる (偽陽性回帰の unit アンカー。
    // 実 mermaid 出力を通す本物の回帰は stories/kfm/KfmMermaid.stories.ts の Click story)
    mermaidMock.render.mockResolvedValueOnce({
      svg: '<svg><a xlink:href="https://example.com"><g class="kfm-mermaid-test-probe"></g></a></svg>',
    });
    const element = await mount('flowchart TD\n  A --> B\n  click A "https://example.com"');
    expect(element.dataset.kfmMermaid).toBe('rendered');
    expect(element.shadowRoot?.querySelector('svg a')).not.toBeNull();
  });

  it('HTML パースで <svg> 根の外へ要素が漏れる出力は error (単一 svg 根の構造検査)', async () => {
    const consoleError = vi.spyOn(console, 'error').mockImplementation(() => undefined);
    mermaidMock.render.mockResolvedValueOnce({
      svg: '<svg></svg><div>svg の外に漏れた要素</div>',
    });
    const element = await mount('flowchart TD\n  A --> B');
    expect(element.dataset.kfmMermaid).toBe('error');
    expect(element.shadowRoot).toBeNull();
    consoleError.mockRestore();
  });

  it('script / on* を含む SVG は shadow DOM へ入れず error へ倒す', async () => {
    const consoleError = vi.spyOn(console, 'error').mockImplementation(() => undefined);
    mermaidMock.render.mockResolvedValueOnce({
      svg: '<svg onload="alert(1)"><script>alert(2)</script></svg>',
    });
    const element = await mount('flowchart TD\n  A --> B');
    expect(element.dataset.kfmMermaid).toBe('error');
    expect(element.shadowRoot).toBeNull();
    expect(consoleError).toHaveBeenCalledWith('[kfm-mermaid] render failed', expect.any(Error));
    consoleError.mockRestore();
  });

  it('描画待ち中に切断された要素は、再接続時に描画を再試行する', async () => {
    let resolveFirstRender!: (value: { svg: string }) => void;
    mermaidMock.render.mockImplementationOnce(
      async () =>
        await new Promise<{ svg: string }>((resolve) => {
          resolveFirstRender = resolve;
        }),
    );

    const element = document.createElement(KFM_MERMAID_TAG);
    element.textContent = 'flowchart TD\n  A --> B';
    document.body.append(element);
    await vi.waitFor(() => expect(mermaidMock.render).toHaveBeenCalledTimes(1));
    element.remove();
    resolveFirstRender({ svg: '<svg><g></g></svg>' });
    expect(element.dataset.kfmMermaid).toBeUndefined();

    document.body.append(element);
    await vi.waitFor(() => expect(element.dataset.kfmMermaid).toBe('rendered'));
    expect(mermaidMock.render).toHaveBeenCalledTimes(2);
  });
});
