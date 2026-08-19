import type { Meta, StoryObj } from '@storybook/vue3-vite';
import { expect } from 'storybook/test';
import classSpoofHtml from '@/lib/kfm-story-fixtures/rendered/sanitize-class-spoof.html?raw';
import inlineStyleHtml from '@/lib/kfm-story-fixtures/rendered/sanitize-inline-style.html?raw';
import scriptHtml from '@/lib/kfm-story-fixtures/rendered/sanitize-script.html?raw';
// class-spoof story は正規 alert を含むため、消費側として CSS サイドカーを明示 import
import '@/lib/remark-koyori-alerts/style.css';
// 器は本番と同じ .kfm-content (単一ソース = content-class.ts)
import { KFM_CONTENT_CLASS } from '@/lib/remark-gfm/content-class';

/*
 * KFM サニタイズの story 群。「通すべきものが通り、通してはならぬものが通らない」の
 * 両側を同じ絵に並べる (消えたことが絵で見える)。
 * fixture は renderDescription の事前生成 HTML (単一ソース = kfm-story-fixtures/inputs.ts、
 * drift 検査 = kfm-story-fixtures.test.ts)。v-html のみの同期描画で VRT が決定的になる。
 */

type KfmStoryArgs = { html: string };

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
  args: { html: scriptHtml },
  parameters: {
    docs: {
      description: {
        story:
          '壊れたら: 「この下から消える:」の直後に何かが現れたら生 HTML が通っている。フェンス内のエスケープ済み script 文字列が消えたら通すべき側が壊れている。',
      },
    },
  },
  play: async ({ canvasElement }) => {
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
  args: { html: inlineStyleHtml },
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
  args: { html: classSpoofHtml },
  parameters: {
    docs: {
      description: {
        story:
          '壊れたら: callout の絵が 2 つに増えたら生 HTML の kfm-alert 騙りが通っている。callout がゼロになったら正規 alert (通すべき側) が壊れている。',
      },
    },
  },
  play: async ({ canvasElement }) => {
    // 正規 alert (プラグイン経由) のちょうど 1 つだけが callout になる
    await expect(canvasElement.querySelectorAll('.kfm-alert')).toHaveLength(1);
    await expect(canvasElement.querySelector('.kfm-alert--note')).not.toBeNull();
    // 騙り側 (caution を名乗る生 HTML) は要素ごと消えている
    await expect(canvasElement.querySelector('.kfm-alert--caution')).toBeNull();
  },
};
