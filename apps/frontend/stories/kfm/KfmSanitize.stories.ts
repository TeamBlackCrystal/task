import type { Meta, StoryObj } from '@storybook/vue3-vite';
import { expect } from 'storybook/test';
import classSpoofHtml from '@/lib/kfm-story-fixtures/rendered/sanitize-class-spoof.html?raw';
import inlineStyleHtml from '@/lib/kfm-story-fixtures/rendered/sanitize-inline-style.html?raw';
import scriptHtml from '@/lib/kfm-story-fixtures/rendered/sanitize-script.html?raw';
import { createSanitizer } from '@/lib/markup-renderer/_sanitize';
// 本番と同じスキーマ一覧の単一ソース (_schemas.ts)。story 側で一覧を組み直さない
import { kfmSanitizeSchemas } from '@/lib/markup-renderer/_schemas';
// class-spoof story は正規 alert を含むため、消費側として CSS サイドカーを明示 import。
// 本番消費側は alerts / GFM の両サイドカーを import するため story も両方揃える
// (現 fixture に GFM 対象要素は無いが、器の CSS 条件は本番と常に一致させる)
import '@/lib/remark-koyori-alerts/style.css';
import '@/lib/remark-gfm/style.css';
// 器は本番と同じ .kfm-content (単一ソース = content-class.ts)
import { KFM_CONTENT_CLASS } from '@/lib/remark-gfm/content-class';

/*
 * KFM サニタイズの story 群。「通すべきものが通り、通してはならぬものが通らない」の
 * 両側を同じ絵に並べる (消えたことが絵で見える)。
 * fixture は renderDescription の事前生成 HTML (単一ソース = kfm-story-fixtures/inputs.ts、
 * drift 検査 = kfm-story-fixtures.test.ts)。本 story 群は fixture に加えて攻撃 probe を
 * module 評価時に同期サニタイズして足す (下の sanitizeStoryProbe)。描画自体は v-html の
 * 同期描画のままで、probe 生成も同期ゆえ VRT の決定性は保たれる。
 */

type KfmStoryArgs = { html: string };

// fixture は本番レンダラ全体の出力を固定する一方、生 Markdown の HTML は DOMPurify より
// 前に remark-rehype が落とす。防御層の story が前段の挙動だけを見ないよう、攻撃 probe は
// 本番と同じ sanitizer 設定へ直接通して fixture に足す。
const sanitizeStoryProbe = createSanitizer(kfmSanitizeSchemas);

const kfmRender = (args: KfmStoryArgs) => ({
  setup: () => ({ args }),
  template: `<div class="${KFM_CONTENT_CLASS}" v-html="args.html" />`,
});

const meta = {
  title: 'KFM/Sanitize',
  tags: ['autodocs'],
  render: kfmRender,
} satisfies Meta<KfmStoryArgs>;

export default meta;
type Story = StoryObj<typeof meta>;

export const ScriptDropped: Story = {
  name: 'script は消える（フェンス内の同文は残る）',
  args: {
    html: `${scriptHtml}${sanitizeStoryProbe(
      '<p data-kfm-sanitize-probe="script">DOMPurify probe<script>alert(1)</script></p>',
    )}`,
  },
  parameters: {
    docs: {
      description: {
        story:
          '壊れたら: 「この下から消える:」の直後に何かが現れたら生 HTML が通っている。フェンス内のエスケープ済み script 文字列が消えたら通すべき側が壊れている。',
      },
    },
  },
  play: async ({ canvasElement }) => {
    await expect(canvasElement.querySelector('[data-kfm-sanitize-probe="script"]')).not.toBeNull();
    // 通してはならぬもの: script 要素は DOM に存在しない
    await expect(canvasElement.querySelector('script')).toBeNull();
    // 通すべきもの: フェンス内の同じ文字列はエスケープ済みテキストとして見えている
    await expect(canvasElement.querySelector('pre code')?.textContent).toContain(
      '<script>alert(1)</script>',
    );
  },
};

export const InlineStyleDropped: Story = {
  name: 'inline style は消える（強調は通る）',
  args: {
    html: `${inlineStyleHtml}${sanitizeStoryProbe(
      '<span data-kfm-sanitize-probe="inline-style" style="position:fixed;color:red">DOMPurify style probe</span>',
    )}`,
  },
  parameters: {
    docs: {
      description: {
        story:
          '壊れたら: 「style 付き生 HTML」の文字が赤く塗られる・画面を覆う要素が現れると style が通っている。強調 (太字) が消えたら通すべき側が壊れている。',
      },
    },
  },
  play: async ({ canvasElement }) => {
    const container = canvasElement.querySelector(`.${KFM_CONTENT_CLASS}`);
    // probe 要素そのものは通し、FORBID_ATTR が style だけを落としたことを見る
    await expect(
      container?.querySelector('[data-kfm-sanitize-probe="inline-style"]'),
    ).not.toBeNull();
    // 通してはならぬもの: style 属性を持つ要素がひとつも無い
    await expect(container?.querySelector('[style]')).toBeNull();
    // 通すべきもの: markdown の強調は要素として生きている
    await expect(container?.querySelector('strong')?.textContent).toBe('強調は通る');
    // inline 生 HTML はタグだけ落ち、テキストは無装飾で残る (絵では黒い素のテキスト)
    await expect(container?.textContent).toContain('style 付き生 HTML');
  },
};

export const ClassSpoofDropped: Story = {
  name: 'アプリ class の騙りは消える（正規 alert は通る）',
  args: {
    html: `${classSpoofHtml}${sanitizeStoryProbe(
      '<div data-kfm-sanitize-probe="class-spoof" class="modal-overlay">DOMPurify class probe</div>',
    )}`,
  },
  parameters: {
    docs: {
      description: {
        story:
          '壊れたら: callout の絵が 2 つに増えたら生 HTML の kfm-alert 騙りが通っている。callout がゼロになったら正規 alert (通すべき側) が壊れている。',
      },
    },
  },
  play: async ({ canvasElement }) => {
    const spoofProbe = canvasElement.querySelector('[data-kfm-sanitize-probe="class-spoof"]');
    await expect(spoofProbe).not.toBeNull();
    await expect(spoofProbe).not.toHaveAttribute('class');
    // 正規 alert (プラグイン経由) のちょうど 1 つだけが callout になる
    await expect(canvasElement.querySelectorAll('.kfm-alert')).toHaveLength(1);
    await expect(canvasElement.querySelector('.kfm-alert--note')).not.toBeNull();
    // 騙り側 (caution を名乗る生 HTML) は要素ごと消えている
    await expect(canvasElement.querySelector('.kfm-alert--caution')).toBeNull();
  },
};
