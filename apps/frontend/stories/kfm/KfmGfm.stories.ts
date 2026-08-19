import type { Meta, StoryObj } from '@storybook/vue3-vite';
import { expect } from 'storybook/test';
import codeFenceHtml from '@/lib/kfm-story-fixtures/rendered/gfm-code-fence.html?raw';
import deepQuoteHtml from '@/lib/kfm-story-fixtures/rendered/gfm-deep-quote.html?raw';
import nestedListsHtml from '@/lib/kfm-story-fixtures/rendered/gfm-nested-lists.html?raw';
import strikeAutolinkHtml from '@/lib/kfm-story-fixtures/rendered/gfm-strike-autolink.html?raw';
import tableAlignmentHtml from '@/lib/kfm-story-fixtures/rendered/gfm-table-alignment.html?raw';
import tableOverflowHtml from '@/lib/kfm-story-fixtures/rendered/gfm-table-overflow.html?raw';
import taskListHtml from '@/lib/kfm-story-fixtures/rendered/gfm-task-list.html?raw';
// CSS サイドカー: レンダラは CSS を import しない契約のため、消費側 (= story) が明示 import。
// GFM CSS は .kfm-content 子孫限定ゆえ、器にも同じクラスを付けて初めて当たる
// (本番消費側と同じ二点契約。単一ソース = content-class.ts)
import { KFM_CONTENT_CLASS } from '@/lib/remark-gfm/content-class';
import '@/lib/remark-gfm/style.css';

/*
 * KFM (md レンダリング) の GFM story 群。
 * fixture は renderDescription の事前生成 HTML (入力の単一ソース =
 * src/lib/kfm-story-fixtures/inputs.ts、drift 検査 = kfm-story-fixtures.test.ts)。
 * 本番と同じく「サーバ生成 HTML を v-html するだけ」なので描画は同期・決定的で、
 * VRT baseline に向く。
 */

type KfmStoryArgs = { html: string };

const kfmRender = (args: KfmStoryArgs) => ({
  setup: () => ({ args }),
  template: `<div class="${KFM_CONTENT_CLASS}" v-html="args.html" />`,
});

const meta = {
  title: 'KFM/GFM',
  tags: ['autodocs'],
  render: kfmRender,
} satisfies Meta<KfmStoryArgs>;

export default meta;
type Story = StoryObj<typeof meta>;

export const TableAlignment: Story = {
  name: '表（列揃え）',
  args: { html: tableAlignmentHtml },
  parameters: {
    docs: {
      description: {
        story:
          '壊れたら: th/td の align 属性が剥がれる (sanitize / remark-rehype の変化) と 3 列の文字寄せが全て左に揃い、絵が変わる。',
      },
    },
  },
};

export const TableOverflow: Story = {
  name: '表（横溢れ）',
  args: { html: tableOverflowHtml },
  // 横溢れを絵にするため、狭い親 (max-w-md) に閉じ込めて描画する
  render: (args: KfmStoryArgs) => ({
    setup: () => ({ args }),
    template: `<div class="max-w-md"><div class="${KFM_CONTENT_CLASS}" v-html="args.html" /></div>`,
  }),
  parameters: {
    docs: {
      description: {
        story:
          '壊れたら: 幅広の表が狭い親をどうはみ出すか (潰れ方・突き抜け方) が変わったら、テーブルレイアウトか消費側 overflow 方針の変化。',
      },
    },
  },
};

export const TaskList: Story = {
  name: 'タスクリスト',
  args: { html: taskListHtml },
  parameters: {
    docs: {
      description: {
        story:
          '壊れたら: checkbox 化が解けて素の [x] テキストに戻る、または contains-task-list / task-list-item class が剥がれて DOM が変わる。CSS サイドカーが無いと bullet が復活し絵も変わる (play で class と checkbox 数を固定)。',
      },
    },
  },
  play: async ({ canvasElement }) => {
    await expect(canvasElement.querySelector('.contains-task-list')).not.toBeNull();
    await expect(canvasElement.querySelectorAll('.task-list-item')).toHaveLength(5);
    await expect(canvasElement.querySelectorAll('input[type="checkbox"]')).toHaveLength(5);
  },
};

export const StrikeAutolink: Story = {
  name: '打ち消し線・自動リンク',
  args: { html: strikeAutolinkHtml },
  parameters: {
    docs: {
      description: {
        story:
          '壊れたら: ~~文~~ が del にならず素のチルダが見える (play で del を固定)、または裸 URL が a にならずリンク色/下線が消えると絵が変わる (CSS サイドカー欠落でも VRT が拾う)。',
      },
    },
  },
  play: async ({ canvasElement }) => {
    await expect(canvasElement.querySelector('del')).not.toBeNull();
    const link = canvasElement.querySelector('a[href^="https://example.com"]');
    await expect(link).not.toBeNull();
    const style = link ? getComputedStyle(link) : null;
    await expect(style?.textDecoration).toContain('underline');
    await expect(style?.color).not.toBe('');
  },
};

export const NestedLists: Story = {
  name: '入れ子のリスト',
  args: { html: nestedListsHtml },
  parameters: {
    docs: {
      description: {
        story:
          '壊れたら: 番号付き/記号リストの 4 段入れ子 (ol > ol > ul > ul) が 1 段に潰れると play の DOM 構造が変わる。CSS サイドカーが無いとマーカー/インデントが消え絵も変わる。',
      },
    },
  },
  play: async ({ canvasElement }) => {
    await expect(canvasElement.querySelectorAll('ol ol ul ul')).toHaveLength(1);
    const outerOl = canvasElement.querySelector('ol');
    await expect(outerOl?.querySelectorAll(':scope > li')).toHaveLength(2);
  },
};

export const DeepQuote: Story = {
  name: '深い引用',
  args: { html: deepQuoteHtml },
  parameters: {
    docs: {
      description: {
        story:
          '壊れたら: 三段の blockquote の入れ子 (blockquote > blockquote > blockquote) が壊れると play が落ちる。CSS サイドカーが無いと縦線が 0 本になり絵も変わる。',
      },
    },
  },
  play: async ({ canvasElement }) => {
    await expect(canvasElement.querySelector('blockquote blockquote blockquote')).not.toBeNull();
    await expect(canvasElement.querySelectorAll('blockquote')).toHaveLength(3);
  },
};

export const CodeFence: Story = {
  name: 'コードフェンス（着色前の素の姿）',
  args: { html: codeFenceHtml },
  parameters: {
    docs: {
      description: {
        story:
          '壊れたら: 着色前の基準。cmd_669 (starry-night) が入ると drift 検査が落ち、fixture 再生成後にトークンが色分かれした絵へ変わる——それがこの story の役目。着色以外で絵が変わったらエスケープか pre/code 構造の変化。',
      },
    },
  },
};
