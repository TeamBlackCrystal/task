import type { Meta, StoryObj } from '@storybook/vue3-vite';
import { expect } from 'storybook/test';
import longLineHtml from '@/lib/kfm-story-fixtures/rendered/code-highlight-long-line.html?raw';
import noLanguageHtml from '@/lib/kfm-story-fixtures/rendered/code-highlight-no-language.html?raw';
import pythonHtml from '@/lib/kfm-story-fixtures/rendered/code-highlight-python.html?raw';
import rustHtml from '@/lib/kfm-story-fixtures/rendered/code-highlight-rust.html?raw';
import typescriptHtml from '@/lib/kfm-story-fixtures/rendered/code-highlight-typescript.html?raw';
import unknownLanguageHtml from '@/lib/kfm-story-fixtures/rendered/code-highlight-unknown-language.html?raw';
import { KFM_CONTENT_CLASS } from '@/lib/remark-gfm/content-class';
// CSS サイドカー: レンダラは CSS を import しない契約のため、消費側 (= story) が明示 import
// (器 .kfm-content のスタイルは GFM サイドカー側 — 他 story と同じ二枚組)
import '@/lib/remark-gfm/style.css';
import '@/lib/rehype-starry-night/style.css';

/*
 * KFM コードブロック着色 (starry-night) の story 群。cmd_670 の fixture+v-html 方式:
 * fixture は renderDescription の事前生成 HTML (単一ソース =
 * src/lib/kfm-story-fixtures/inputs.ts、drift 検査 = kfm-story-fixtures.test.ts)。
 * v-html のみの同期描画で VRT が決定的になる。
 */

type KfmStoryArgs = { html: string };

const kfmRender = (args: KfmStoryArgs) => ({
  setup: () => ({ args }),
  template: `<div class="${KFM_CONTENT_CLASS}" v-html="args.html" />`,
});

const meta = {
  title: 'KFM/CodeHighlight',
  tags: ['autodocs'],
  render: kfmRender,
} satisfies Meta<KfmStoryArgs>;

export default meta;
type Story = StoryObj<typeof meta>;

/** 着色経路が生きていることの共通 assert (pl- クラスの span が存在する) */
async function expectHighlighted(canvasElement: HTMLElement) {
  await expect(canvasElement.querySelector('[class^="pl-"]')).not.toBeNull();
}

export const TypeScript: Story = {
  name: 'TypeScript（着色）',
  args: { html: typescriptHtml },
  parameters: {
    docs: {
      description: {
        story:
          '壊れたら: キーワード/型注釈/テンプレート literal の色分かれが消えて単色になったら、rehype 層か sanitize の pl- 許可の崩れ。',
      },
    },
  },
  play: async ({ canvasElement }) => {
    await expectHighlighted(canvasElement);
    await expect(canvasElement.querySelector('code.language-ts')).not.toBeNull();
  },
};

export const Rust: Story = {
  name: 'Rust（着色）',
  args: { html: rustHtml },
  parameters: {
    docs: {
      description: {
        story:
          '壊れたら: fn/let/println! マクロの色分かれが消えたら rust 文法の脱落 (common セット変更) か着色経路の崩れ。',
      },
    },
  },
  play: async ({ canvasElement }) => {
    await expectHighlighted(canvasElement);
    await expect(canvasElement.querySelector('code.language-rust')).not.toBeNull();
  },
};

export const Python: Story = {
  name: 'Python（着色）',
  args: { html: pythonHtml },
  parameters: {
    docs: {
      description: {
        story:
          '壊れたら: def/docstring/f-string/コメントの色分かれが消えたら python 文法の脱落か着色経路の崩れ。',
      },
    },
  },
  play: async ({ canvasElement }) => {
    await expectHighlighted(canvasElement);
    await expect(canvasElement.querySelector('code.language-python')).not.toBeNull();
  },
};

export const NoLanguage: Story = {
  name: '言語指定なし（素の姿）',
  args: { html: noLanguageHtml },
  parameters: {
    docs: {
      description: {
        story:
          '壊れたら: 言語なしフェンスに色が付いたら「指定した言語だけ着色する」境界の崩れ。テキストがタグとして描画されたらエスケープの崩れ。',
      },
    },
  },
  play: async ({ canvasElement }) => {
    await expect(canvasElement.querySelector('[class^="pl-"]')).toBeNull();
    await expect(canvasElement.querySelector('code')?.getAttribute('class')).toBeNull();
    // コードテキスト内のタグ表記はテキストのまま (b 要素は生まれない)
    await expect(canvasElement.querySelector('code b')).toBeNull();
    await expect(canvasElement.querySelector('code')?.textContent).toContain('<b>');
  },
};

export const UnknownLanguage: Story = {
  name: '未知言語（素のフォールバック）',
  args: { html: unknownLanguageHtml },
  parameters: {
    docs: {
      description: {
        story:
          '壊れたら: 未知言語で描画が空になったり色が付いたらフォールバックの崩れ。内容がタグとして出たらエスケープの崩れ。',
      },
    },
  },
  play: async ({ canvasElement }) => {
    await expect(canvasElement.querySelector('[class^="pl-"]')).toBeNull();
    await expect(canvasElement.querySelector('code.language-definitelynotalang')).not.toBeNull();
    await expect(canvasElement.querySelector('code b')).toBeNull();
    await expect(canvasElement.querySelector('code')?.textContent).toContain('<b>');
  },
};

export const LongLine: Story = {
  name: '横に長い行（横溢れ）',
  args: { html: longLineHtml },
  // 横溢れを絵にするため、狭い親 (max-w-md) に閉じ込めて描画する (cmd_670 の表と同形)
  render: (args: KfmStoryArgs) => ({
    setup: () => ({ args }),
    template: `<div class="max-w-md"><div class="${KFM_CONTENT_CLASS}" v-html="args.html" /></div>`,
  }),
  parameters: {
    docs: {
      description: {
        story:
          '壊れたら: 長い 1 行の pre が狭い親をどうはみ出すか (折返し/突き抜け) が変わったら、pre の overflow 方針か消費側スタイルの変化。',
      },
    },
  },
  play: async ({ canvasElement }) => {
    await expectHighlighted(canvasElement);
    const pre = canvasElement.querySelector('pre');
    await expect(pre).not.toBeNull();
    if (!(pre instanceof HTMLElement)) throw new Error('LongLine story に pre が無い');
    // Story 名や説明だけでなく、狭い器に対する実寸で横溢れを主張する。
    await expect(pre.scrollWidth).toBeGreaterThan(pre.clientWidth);
  },
};

export const DarkTheme: Story = {
  name: 'ダークテーマ（ライトと並置）',
  args: { html: typescriptHtml },
  // 同一 fixture をライト地とダーク地 (.dark ancestor = アプリ本体と同方式) で並置。
  // style.css の .dark ブリッジ (starry-night dark パレット) が効いていることが絵で見える。
  render: (args: KfmStoryArgs) => ({
    setup: () => ({ args }),
    template:
      '<div class="grid gap-4">' +
      `<div class="p-4"><div class="${KFM_CONTENT_CLASS} kfm-story-light" v-html="args.html" /></div>` +
      `<div class="dark bg-background text-foreground p-4"><div class="${KFM_CONTENT_CLASS} kfm-story-dark" v-html="args.html" /></div>` +
      '</div>',
  }),
  parameters: {
    docs: {
      description: {
        story:
          '壊れたら: 上下の箱でコードの配色が同じになったら .dark ブリッジ (style.css) の喪失——alerts の「CSS ライト固定」と同型の再発をこの絵で検知する。',
      },
    },
  },
  play: async ({ canvasElement }) => {
    // 同じトークン (キーワード) の computed color がライト箱とダーク箱で異なることを
    // 機械で固定する (パレット実値はハードコードせず相対比較 = upstream 更新に強い)
    const lightKeyword = canvasElement.querySelector('.kfm-story-light .pl-k');
    const darkKeyword = canvasElement.querySelector('.kfm-story-dark .pl-k');
    await expect(lightKeyword).not.toBeNull();
    await expect(darkKeyword).not.toBeNull();
    const lightColor = getComputedStyle(lightKeyword as Element).color;
    const darkColor = getComputedStyle(darkKeyword as Element).color;
    await expect(lightColor).not.toBe('');
    await expect(darkColor).not.toBe('');
    await expect(lightColor).not.toBe(darkColor);
  },
};
