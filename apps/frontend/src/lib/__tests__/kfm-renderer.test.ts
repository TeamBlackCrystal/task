import { describe, expect, it } from 'vitest';
import { createRenderer, renderDescription } from '../markup-renderer';
import type { KfmProfile } from '../markup-renderer';
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
});
