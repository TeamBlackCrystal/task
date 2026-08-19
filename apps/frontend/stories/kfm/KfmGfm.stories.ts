import type { Meta, StoryObj } from '@storybook/vue3-vite';
import codeFenceHtml from '@/lib/kfm-story-fixtures/rendered/gfm-code-fence.html?raw';
import deepQuoteHtml from '@/lib/kfm-story-fixtures/rendered/gfm-deep-quote.html?raw';
import nestedListsHtml from '@/lib/kfm-story-fixtures/rendered/gfm-nested-lists.html?raw';
import strikeAutolinkHtml from '@/lib/kfm-story-fixtures/rendered/gfm-strike-autolink.html?raw';
import tableAlignmentHtml from '@/lib/kfm-story-fixtures/rendered/gfm-table-alignment.html?raw';
import tableOverflowHtml from '@/lib/kfm-story-fixtures/rendered/gfm-table-overflow.html?raw';
import taskListHtml from '@/lib/kfm-story-fixtures/rendered/gfm-task-list.html?raw';

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
  template: '<div class="kfm-story" v-html="args.html" />',
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
    template: '<div class="max-w-md"><div class="kfm-story" v-html="args.html" /></div>',
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
          '壊れたら: checkbox 化が解けて素の [x] テキストに戻る、または contains-task-list / task-list-item class が剥がれてインデントが崩れると絵が変わる。',
      },
    },
  },
};

export const StrikeAutolink: Story = {
  name: '打ち消し線・自動リンク',
  args: { html: strikeAutolinkHtml },
  parameters: {
    docs: {
      description: {
        story:
          '壊れたら: ~~文~~ が del にならず素のチルダが見える、または裸 URL が a にならずリンク色/下線が消えると絵が変わる。',
      },
    },
  },
};

export const NestedLists: Story = {
  name: '入れ子のリスト',
  args: { html: nestedListsHtml },
  parameters: {
    docs: {
      description: {
        story:
          '壊れたら: 番号付き/記号リストの 4 段の入れ子でインデント幅やマーカー種別が変わると絵が変わる (parse かリストスタイルの変化)。',
      },
    },
  },
};

export const DeepQuote: Story = {
  name: '深い引用',
  args: { html: deepQuoteHtml },
  parameters: {
    docs: {
      description: {
        story: '壊れたら: 三段の blockquote の縦線が 3 本未満になったら引用の入れ子が壊れている。',
      },
    },
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
