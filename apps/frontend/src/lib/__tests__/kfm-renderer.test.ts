import type { Root } from 'mdast';
import { describe, expect, it } from 'vitest';
import { createRenderer, renderDescription } from '../markup-renderer';
import type { CreateRendererOptions, KfmProfile } from '../markup-renderer';
import { createL1Cache } from '../markup-renderer/_cache';
import { gfmSanitizeSchema, remarkGfm } from '../remark-gfm';
import { koyoriAlertsSanitizeSchema, remarkKoyoriAlerts } from '../remark-koyori-alerts';

const ALERT_TYPES = ['note', 'tip', 'important', 'warning', 'caution'] as const;

function createGithubRenderer() {
  return createRenderer({
    profiles: { github: { remarkPlugins: [remarkGfm, remarkKoyoriAlerts] } },
    sanitizeSchemas: [gfmSanitizeSchema, koyoriAlertsSanitizeSchema],
  });
}

describe('renderDescription (GFM 基本)', () => {
  it('テーブルを描画する', async () => {
    const html = await renderDescription('| a | b |\n| - | - |\n| 1 | 2 |');
    expect(html).toContain('<table>');
    expect(html).toContain('<td>1</td>');
  });

  it('打ち消し線を描画する', async () => {
    const html = await renderDescription('~~消した~~');
    expect(html).toContain('<del>消した</del>');
  });

  it('タスクリストを class 付きで描画する (GFM 由来 class が sanitize を通る陽性対照)', async () => {
    const html = await renderDescription('- [x] 済み\n- [ ] 未了');
    expect(html).toContain('task-list-item');
    expect(html).toContain('contains-task-list');
    expect(html).toContain('type="checkbox"');
    expect(html).toContain('checked');
  });

  it('autolink をリンク化する', async () => {
    const html = await renderDescription('本文 https://example.com/path を参照');
    expect(html).toContain('<a href="https://example.com/path">');
  });

  it('脚注を描画する', async () => {
    const html = await renderDescription('本文[^1]\n\n[^1]: 脚注内容');
    expect(html).toContain('脚注内容');
    expect(html).toContain('footnotes');
  });

  it('コードフェンスの language-* class が残る', async () => {
    const html = await renderDescription('```ts\nconst x = 1;\n```');
    expect(html).toContain('language-ts');
  });

  // yupix 実測 5 例の回帰固定: language-* パターンから大文字・+・# のいずれかを
  // 落とす変更はここで赤くなる (starry-night は language-* class から言語を知るため、
  // class が剥がれると C++/C#/大文字表記の言語は着色されない)。
  it.each(['C++', 'c#', 'TS', 'JSON', 'objective-c'])(
    '```%s の language-* class が sanitize を通る',
    async (lang) => {
      const html = await renderDescription(`\`\`\`${lang}\nx\n\`\`\``);
      expect(html).toContain(`language-${lang}`);
    },
  );
});

describe('renderDescription (GitHub alerts 境界)', () => {
  it.each(ALERT_TYPES)('[!%s] を callout に変換する', async (type) => {
    const marker = type.toUpperCase();
    const html = await renderDescription(`> [!${marker}]\n> 本文です`);
    expect(html).toContain(`kfm-alert--${type}`);
    expect(html).toContain('kfm-alert__title');
    expect(html).toContain('本文です');
    expect(html).not.toContain('<blockquote>');
  });

  it('type は case-insensitive (小文字マーカーでも callout 化)', async () => {
    const html = await renderDescription('> [!note]\n> 小文字テスト');
    expect(html).toContain('kfm-alert--note');
  });

  it('タイトル語 (Note 等) を title 段落として出す', async () => {
    const html = await renderDescription('> [!NOTE]\n> 本文');
    expect(html).toContain('>Note</p>');
  });

  it('不正 type ([!HINT]) は通常 blockquote のまま', async () => {
    const html = await renderDescription('> [!HINT]\n> ヒント本文');
    expect(html).toContain('<blockquote>');
    expect(html).not.toContain('kfm-alert');
  });

  it('マーカー行末のスペース 2 つ (hard break) でも callout 化する', async () => {
    // GitHub は `> [!WARNING]␣␣` (行末スペース 2 つ = hard break) も alert にする。
    // hard break は mdast で独立した break ノードになるため、text 内改行とも
    // 同一行後続テキストとも別扱いが要る (対: 下の同一行後続テキスト 2 試験)。
    const html = await renderDescription('> [!WARNING]  \n> 注意');
    expect(html).toContain('kfm-alert--warning');
    expect(html).toContain('注意');
    expect(html).not.toContain('<blockquote>');
    // マーカー由来の hard break が本文先頭へ漏れて <br> にならないこと
    expect(html).not.toContain('<br');
  });

  it('マーカーと同一行に後続テキストがあれば通常 blockquote のまま', async () => {
    const html = await renderDescription('> [!NOTE] 同じ行の続き\n> 次の行');
    expect(html).toContain('<blockquote>');
    expect(html).not.toContain('kfm-alert');
  });

  it('マーカーと同一行に inline 構文が続く場合も通常 blockquote のまま', async () => {
    const html = await renderDescription('> [!NOTE] **強調**');
    expect(html).toContain('<blockquote>');
    expect(html).not.toContain('kfm-alert');
  });

  it('複数段落 blockquote は 2 段落目以降を alert 本文として保持する', async () => {
    const html = await renderDescription('> [!TIP]\n>\n> 一段落目\n>\n> 二段落目');
    expect(html).toContain('kfm-alert--tip');
    expect(html).toContain('一段落目');
    expect(html).toContain('二段落目');
  });

  it('ネストした alert は不可 (内側は通常 blockquote のまま)', async () => {
    const html = await renderDescription('> [!NOTE]\n> 外側本文\n>\n> > [!TIP]\n> > 内側本文');
    expect(html).toContain('kfm-alert--note');
    // 内側は alert 化されず、マーカーは素のテキストとして残る (GitHub 実挙動と同じ)
    expect(html).not.toContain('kfm-alert--tip');
    expect(html).toContain('<blockquote>');
    expect(html).toContain('[!TIP]');
  });

  it('素の blockquote 内の > > [!NOTE] も alert 化しない (ネスト不可の対側)', async () => {
    // 対の試験: 上の alert-in-alert (SKIP 側) だけでは「alert でない blockquote の内側」
    // が塞がっていることを保証しない。両側を必ず残すこと。
    const html = await renderDescription('> outer\n>\n> > [!NOTE]\n> > inner');
    expect(html).not.toContain('kfm-alert');
    expect(html).toContain('<blockquote>');
    expect(html).toContain('[!NOTE]');
    expect(html).toContain('inner');
  });

  it('エスケープした \\[!NOTE] は alert 化せず素の blockquote のまま (行も消えない)', async () => {
    // GitHub は \[!NOTE] を素の blockquote ＋ literal な [!NOTE] として描く。
    // remark-parse 済みの text 値ではエスケープが解決済みのため、原文照合で弾く。
    const html = await renderDescription('> \\[!NOTE]\n> 本文');
    expect(html).toContain('<blockquote>');
    expect(html).not.toContain('kfm-alert');
    // マーカー行が黙って消えないこと
    expect(html).toContain('[!NOTE]');
    expect(html).toContain('本文');
  });

  it('CRLF 本文の [!NOTE] が literal のまま残らず callout 化する', async () => {
    // GitHub Issue 本文は CRLF が一般的 (#578 GitHub Issue 同期経路)。LF 入力しか
    // 見ない試験だけでは、行末 \r でマーカー照合が不成立になる退行を検出できない。
    const html = await renderDescription('> [!NOTE]\r\n> CRLF 本文');
    expect(html).toContain('kfm-alert--note');
    expect(html).toContain('CRLF 本文');
    expect(html).not.toContain('[!NOTE]');
    expect(html).not.toContain('<blockquote>');
  });
});

describe('renderDescription (改行コード不変条件: LF と CRLF は同一 HTML)', () => {
  // 不変条件: 同じ文書の LF 版と CRLF 版は同一の HTML を produce する。
  // CRLF 版は LF 版から機械的に導出し、差が改行コードのみであることを構成で保証する。
  it.each([
    ['alert (本文複数行)', '> [!NOTE]\n> 一行目\n> 二行目'],
    ['表', '| a | b |\n| - | - |\n| 1 | 2 |'],
    ['入れ子リスト', '- 親\n  - 子\n    - 孫\n- 次'],
    ['コードフェンス', '```ts\nconst x = 1;\nconst y = 2;\n```'],
    ['脚注', '本文[^1]\n\n[^1]: 脚注内容'],
  ])('%s: LF 版と CRLF 版は同一 HTML', async (_label, lfSource) => {
    const crlfSource = lfSource.replaceAll('\n', '\r\n');
    expect(await renderDescription(crlfSource)).toBe(await renderDescription(lfSource));
  });

  it('旧 Mac 形式の lone CR も LF 版と同一 HTML (正規化を \\r\\n 限定に狭めると赤)', async () => {
    // micromark は lone \r も行末として解釈するため、\r\n と同じ「text 値に原文改行が
    // 残る」穴を持つ。正規化正規表現を /\r\n/ に狭める変更はここで落ちる。
    const lfSource = '> [!NOTE]\n> 一行目\n> 二行目';
    expect(await renderDescription(lfSource.replaceAll('\n', '\r'))).toBe(
      await renderDescription(lfSource),
    );
  });

  it('コードフェンス内容は CR 正規化で壊れない (中身が LF で完全に残る)', async () => {
    const html = await renderDescription('```ts\r\nconst x = 1;\r\nconst y = 2;\r\n```');
    // 着色層 (starry-night) が入るとトークンが span へ分割され、本文は連続した
    // 文字列として現れない。この試験の主眼は CR 正規化であって着色の有無ではないので、
    // タグを外した本文で比べる (着色が有っても無くても成り立つ形)。
    const text = html.replace(/<[^>]+>/g, '');
    expect(text).toContain('const x = 1;\nconst y = 2;\n');
    expect(html).not.toContain('\r');
  });

  it('LF 版と CRLF 版は同一キャッシュエントリに畳まれる (正規化はキー構築より前)', async () => {
    // 入口正規化がキャッシュキー構築より後ろへ移動すると、同一文書が改行コード違いで
    // 別エントリを占有する退行になる。エントリ数で機械検証する。
    const cache = createL1Cache();
    const render = createRenderer({
      profiles: { github: { remarkPlugins: [remarkGfm, remarkKoyoriAlerts] } },
      sanitizeSchemas: [gfmSanitizeSchema, koyoriAlertsSanitizeSchema],
      cache,
    });
    await render('> [!NOTE]\n> 本文');
    expect(cache.size).toBe(1);
    await render('> [!NOTE]\r\n> 本文');
    expect(cache.size).toBe(1);
  });
});

describe('renderDescription (安全 core)', () => {
  it('markdown 中の生 HTML は黙って消える (allowDangerousHtml 不使用)', async () => {
    const html = await renderDescription(
      '前\n\n<div style="color:red" class="modal-overlay">生HTML</div>\n\n後',
    );
    expect(html).toContain('前');
    expect(html).toContain('後');
    expect(html).not.toContain('style=');
    expect(html).not.toContain('modal-overlay');
    expect(html).not.toContain('生HTML');
  });

  it('block の script は丸ごと消える', async () => {
    const html = await renderDescription('前\n\n<script>alert(1)</script>\n\n後');
    expect(html).not.toContain('<script');
    expect(html).not.toContain('alert(1)');
  });

  it('inline の script はタグが消え、中身は実行不能な平文としてのみ残る', async () => {
    const html = await renderDescription('こちら <script>alert(1)</script> です');
    expect(html).not.toContain('<script');
    // inline raw HTML はタグ 2 つの html ノードに分割され (中間テキストは text ノード)、
    // html ノードだけが落ちる。平文 alert(1) は <p> 内テキストで実行経路を持たない。
    expect(html).toContain('<p>こちら alert(1) です</p>');
  });

  it('MFM 構文 ($[...] / :emoji: / @mention / #hashtag) は Phase 1 では素通し (装飾しない)', async () => {
    const html = await renderDescription('$[spin 回る] と :smile: と @user と #tag');
    expect(html).toContain('$[spin 回る]');
    expect(html).toContain(':smile:');
    expect(html).toContain('@user');
    expect(html).toContain('#tag');
    expect(html).not.toContain('kfm-fn');
  });
});

describe('renderDescription (sanitize 結線)', () => {
  // 生 HTML は remark-rehype 既定 (allowDangerousHtml 不使用) の段階で消えるため、
  // 「安全 core」の試験群は sanitize(...) 呼び出しを外しても緑のまま = 結線を検証しない。
  // ここでは remark プラグインの正規 emit 経路 (data.hName / hProperties) から
  // 「sanitize でしか落ちないもの」を最終 HTML へ流し込み、renderDescription の出力が
  // 実際に sanitize を通っていることを固定する。_renderer.ts の sanitize(...) 呼び出しを
  // 外すとこの describe が赤くなる (変異試験で実測済み)。
  function remarkUnsafeEmitter() {
    return (tree: Root) => {
      tree.children.push(
        {
          type: 'paragraph',
          data: {
            hName: 'div',
            hProperties: {
              onclick: 'alert(1)',
              style: 'position:fixed',
              class: 'evil-class kfm-alert',
            },
          },
          children: [{ type: 'text', value: 'emit 本文' }],
        },
        {
          type: 'paragraph',
          data: { hName: 'script' },
          children: [{ type: 'text', value: 'alert(2)' }],
        },
      );
    };
  }

  function createRendererWithUnsafeEmitter() {
    return createRenderer({
      profiles: {
        github: { remarkPlugins: [remarkGfm, remarkKoyoriAlerts, remarkUnsafeEmitter] },
      },
      sanitizeSchemas: [gfmSanitizeSchema, koyoriAlertsSanitizeSchema],
    });
  }

  it('emit 由来の script はタグも中身も最終出力に残らない', async () => {
    const html = await createRendererWithUnsafeEmitter()('本文');
    expect(html).not.toContain('<script');
    expect(html).not.toContain('alert(2)');
  });

  it('emit 由来の on* / style 属性は最終出力に残らない (要素と本文は残る陽性対照)', async () => {
    const html = await createRendererWithUnsafeEmitter()('本文');
    expect(html).toContain('emit 本文');
    expect(html).not.toContain('onclick');
    expect(html).not.toContain('style=');
  });

  it('emit 由来の未許可 class は落ち、許可 class だけ残る (class allowlist の結線)', async () => {
    const html = await createRendererWithUnsafeEmitter()('本文');
    expect(html).not.toContain('evil-class');
    expect(html).toContain('class="kfm-alert"');
  });
});

describe('renderDescription (決定性・profile seam)', () => {
  it('同一入力は独立 renderer 間で同一 HTML (SSR/CSR 同一性)', async () => {
    const input = '# 見出し\n\n> [!WARNING]\n> 注意\n\n- [x] done\n\n`code` と ~~strike~~';
    const first = await createGithubRenderer()(input);
    const second = await createGithubRenderer()(input);
    expect(first).toBe(second);
  });

  it('未構成 profile はエラーになる (seam は fail-closed)', async () => {
    await expect(renderDescription('本文', { profile: 'kfm' as KfmProfile })).rejects.toThrow(
      'profile "kfm" is not configured',
    );
  });

  it('defaultProfile が profile 未指定の描画を実際に変える', async () => {
    // KfmProfile は Phase 1 で 'github' のみのため、第二 profile はテスト内 cast で足す
    const profiles = {
      github: { remarkPlugins: [remarkGfm, remarkKoyoriAlerts] },
      kfm: { remarkPlugins: [] },
    } as unknown as CreateRendererOptions['profiles'];
    const render = createRenderer({
      profiles,
      sanitizeSchemas: [gfmSanitizeSchema, koyoriAlertsSanitizeSchema],
      defaultProfile: 'kfm' as KfmProfile,
    });
    // 既定が kfm (プラグイン無し) になった = GFM の打ち消し線が効かない
    const html = await render('~~strike~~');
    expect(html).not.toContain('<del>');
    expect(html).toContain('~~strike~~');
    // 明示指定は引き続き defaultProfile より勝つ
    const githubHtml = await render('~~strike~~', { profile: 'github' });
    expect(githubHtml).toContain('<del>strike</del>');
  });
});

describe('renderDescription (脚注 fn-* / fnref-* id の scope)', () => {
  const FOOTNOTE = '本文[^1]\n\n[^1]: 脚注内容';

  it('scope 未指定は fn-* / fnref-* とも GitHub 既定 prefix のまま', async () => {
    const html = await renderDescription(FOOTNOTE);
    expect(html).toContain('id="user-content-fn-1"');
    expect(html).toContain('id="user-content-fnref-1"');
  });

  it('scope 違いは fn-* / fnref-* id を分離する', async () => {
    const first = await renderDescription(FOOTNOTE, { scope: 'task-1' });
    const second = await renderDescription(FOOTNOTE, { scope: 'comment-2' });
    expect(first).toContain('id="user-content-task-1-fn-1"');
    expect(first).toContain('id="user-content-task-1-fnref-1"');
    expect(second).toContain('id="user-content-comment-2-fn-1"');
    expect(second).toContain('id="user-content-comment-2-fnref-1"');
    expect(second).not.toContain('user-content-task-1-');
  });

  // この試験は元は「footnote-label は scope が効かず断片間で重複する」という
  // 当時の現状を固定するものだった。本層 (rehype-scope-footnote-label) がその穴を
  // 塞いだため、赤くなることで更新を強制する形で働いた。いまは塞がった後を固定する。
  it('scope 違いなら footnote-label の id と aria-describedby も分かれる', async () => {
    const first = await renderDescription(FOOTNOTE, { scope: 'task-1' });
    const second = await renderDescription(FOOTNOTE, { scope: 'comment-2' });
    expect(first).toContain('id="user-content-task-1-footnote-label"');
    expect(second).toContain('id="user-content-comment-2-footnote-label"');
    expect(first).toContain('aria-describedby="user-content-task-1-footnote-label"');
    expect(second).toContain('aria-describedby="user-content-comment-2-footnote-label"');
    // 素の footnote-label が残っていれば書き換えの取り零しである
    expect(first).not.toContain('"footnote-label"');
    expect(second).not.toContain('"footnote-label"');
  });

  it('同一 scope は決定的 (独立 renderer 間で同一 HTML = L1 キャッシュ前提を壊さない)', async () => {
    const first = await createGithubRenderer()(FOOTNOTE, { scope: 'task-1' });
    const second = await createGithubRenderer()(FOOTNOTE, { scope: 'task-1' });
    expect(first).toBe(second);
  });

  it('id / URL fragment に安全でない scope は fail-closed で throw', async () => {
    await expect(renderDescription(FOOTNOTE, { scope: 'a b' })).rejects.toThrow('scope');
    await expect(renderDescription(FOOTNOTE, { scope: '' })).rejects.toThrow('scope');
  });

  it.each(['fn', 'fnref', 'fn-1', 'a-fn-b', 'a-fnref', 'comment-fn'])(
    'fn / fnref セグメントを含む scope "%s" は throw (細工 label との id 衝突防止)',
    async (scope) => {
      // 例: scope 'a-fn-b' の label 'c' と scope 'a' の label 'b-fn-c' はどちらも
      // id user-content-a-fn-b-fn-c になり scope 分離が破れる。scope 側から
      // fn / fnref セグメントを禁止することで、細工した脚注 label でも衝突しない。
      await expect(renderDescription(FOOTNOTE, { scope })).rejects.toThrow('fn');
    },
  );

  it('fn / fnref を部分文字列として含むだけのセグメントは通る (fnord 等)', async () => {
    const html = await renderDescription(FOOTNOTE, { scope: 'my-fnord-1' });
    expect(html).toContain('id="user-content-my-fnord-1-fn-1"');
  });

  it('細工 label と別 scope の id は衝突しない (scope 分離の不変条件)', async () => {
    // scope 'a' + 細工 label 'b-fn-1' が作る id は user-content-a-fn-b-fn-1。
    // これと衝突し得た scope 'a-fn-b' は上の検証で throw 済みのため、生成側の id を
    // 名指しで固定して不変条件を機械検証する。
    const crafted = await renderDescription('本文[^b-fn-1]\n\n[^b-fn-1]: 細工', { scope: 'a' });
    expect(crafted).toContain('id="user-content-a-fn-b-fn-1"');
  });
});

describe('renderDescription (ユーザーリンクの現状契約)', () => {
  it('リンクは target を持たない (rel 硬化を Phase 1 で行わない前提条件の固定)', async () => {
    // 仕様: target="_blank" を出さないため reverse tabnabbing 経路が無く、rel 硬化は
    // 行わない (docs/frontend/kfm-phase1-implementation.md)。target を導入する変更は
    // ここで赤くなり、rel="noopener noreferrer" の同時導入を強制する。
    const html = await renderDescription('[リンク](https://example.com/) と https://example.org/');
    expect(html).toContain('<a href="https://example.com/">');
    expect(html).not.toContain('target=');
    expect(html).not.toContain('rel=');
  });
});

describe('renderDescription (scope 不変条件: 断片連結で全 id 一意)', () => {
  // 脚注に限らず「id を出しうる要素」を広く含む入力。見出し・タスクリスト・コード
  // フェンス・mermaid・テーブル・alert は今は id を出さないが、将来 id を出す層
  // (rehype-slug 等) が固定 id のまま合流したらこの不変条件が赤くなる。
  const RICH = [
    '# 見出し anchor 候補',
    '本文[^1] と別脚注[^note] を参照',
    '- [x] 済みタスク\n- [ ] 未了タスク',
    '```ts\nconst x = 1;\n```',
    '```mermaid\ngraph TD; A-->B\n```',
    '| a |\n| - |\n| 1 |',
    '> [!NOTE]\n> alert 本文',
    '[^1]: 一つ目の脚注',
    '[^note]: 二つ目の脚注',
  ].join('\n\n');

  function extractIds(html: string): string[] {
    return [...html.matchAll(/ id="([^"]*)"/g)].map((m) => m[1]);
  }

  function extractAriaDescribedBy(html: string): string[] {
    return [...html.matchAll(/ aria-describedby="([^"]*)"/g)].flatMap((m) => m[1].split(' '));
  }

  it('異なる scope で描画した出力を連結しても全 id が一意 (footnote-label 含む)', async () => {
    const fragments = await Promise.all([
      renderDescription(RICH, { scope: 'task-1' }),
      renderDescription(RICH, { scope: 'comment-2' }),
      renderDescription(RICH, { scope: 'comment-3' }),
    ]);
    const ids = fragments.flatMap(extractIds);
    // 陽性対照: sanitize が id を落とすと「id ゼロ→重複ゼロ」で偽の緑になるため、
    // 断片ごとに fn-1 / fn-note / fnref-1 / fnref-note / footnote-label の 5 個以上を要求
    expect(ids.length).toBeGreaterThanOrEqual(fragments.length * 5);
    expect(new Set(ids).size).toBe(ids.length);
  });

  it('断片内で aria-describedby と href="#..." の参照が閉じる (片側書き換えの取り零し検知)', async () => {
    const html = await renderDescription(RICH, { scope: 'task-9' });
    const ids = new Set(extractIds(html));
    const describedBy = extractAriaDescribedBy(html);
    // 陽性対照: sanitize が aria-describedby を落とすと空集合で偽の緑になる
    expect(describedBy.length).toBeGreaterThan(0);
    for (const token of describedBy) {
      expect(ids).toContain(token);
    }
    const fragmentRefs = [...html.matchAll(/ href="#([^"]+)"/g)].map((m) => m[1]);
    expect(fragmentRefs.length).toBeGreaterThan(0);
    for (const ref of fragmentRefs) {
      expect(ids).toContain(ref);
    }
  });

  it('scope 付きは footnote-label の id と aria-describedby が双方 scope 付きになる', async () => {
    const html = await renderDescription(RICH, { scope: 'task-1' });
    expect(html).toContain('id="user-content-task-1-footnote-label"');
    expect(html).toContain('aria-describedby="user-content-task-1-footnote-label"');
    expect(html).not.toContain('id="footnote-label"');
    expect(html).not.toContain('aria-describedby="footnote-label"');
  });

  it('scope 無し (既定) は GitHub 互換 footnote-label のまま (id / aria-describedby 双方)', async () => {
    const html = await renderDescription(RICH);
    expect(html).toContain('id="footnote-label"');
    expect(html).toContain('aria-describedby="footnote-label"');
  });
});
