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

// アプリ本体と同じ .dark ancestor class 方式 (tailwind.css の @custom-variant dark)。
// 背景/文字色もアプリのテーマトークンで塗って実際のダーク画面と同じ地の上で撮る
// (KfmAlerts.stories.ts の kfmDarkRender と同じ形)。
const kfmDarkRender = (args: KfmStoryArgs) => ({
  setup: () => ({ args }),
  template: `<div class="dark bg-background text-foreground p-4"><div class="${KFM_CONTENT_CLASS}" v-html="args.html" /></div>`,
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
          '壊れたら: th/td の align 属性が剥がれる (sanitize / remark-rehype の変化) と 3 列の文字寄せが全て左に揃い、絵が変わる (play で computed textAlign を固定)。なお表の罫線 (セル border) は未実装で、線の無い絵が現状の仕様。',
      },
    },
  },
  play: async ({ canvasElement }) => {
    // align 属性はブラウザが text-align へ写して初めて絵になる。属性剥がれを
    // DOM でなく computed で見ることで「属性はあるが効いていない」も一緒に固定する。
    // Chromium は属性由来の値を -webkit-left 等で返すため接頭辞を剥いで比較する
    const cells = canvasElement.querySelectorAll('tbody tr:first-child td');
    await expect(cells).toHaveLength(3);
    const aligns = Array.from(cells).map((cell) =>
      getComputedStyle(cell).textAlign.replace('-webkit-', ''),
    );
    await expect(aligns).toEqual(['left', 'center', 'right']);
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
          '壊れたら: 幅広の表が狭い親をどうはみ出すか (潰れ方・突き抜け方) が変わったら、テーブルレイアウトか消費側 overflow 方針の変化 (play で幅制限の実効と表/親の幅関係を固定)。なお表の罫線 (セル border) は未実装で、線の無い絵が現状の仕様。',
      },
    },
  },
  play: async ({ canvasElement }) => {
    const container = canvasElement.querySelector('.max-w-md');
    const table = canvasElement.querySelector('table');
    await expect(container).not.toBeNull();
    await expect(table).not.toBeNull();
    // 幅制限の実効 (Tailwind max-w-md = 28rem)。utility が当たらなければ「狭い親」の前提が崩れる
    await expect(container ? getComputedStyle(container).maxWidth : '').toBe('448px');
    // 表は親の幅制限に収まる (auto layout が列を圧縮する潰れ方の絵)。
    // はみ出す絵に変わったらテーブルレイアウトか overflow 方針の変化
    const containerWidth = container?.getBoundingClientRect().width ?? 0;
    const tableWidth = table?.getBoundingClientRect().width ?? 0;
    await expect(containerWidth).toBeGreaterThan(0);
    await expect(tableWidth).toBeLessThanOrEqual(containerWidth);
  },
};

export const TaskList: Story = {
  name: 'タスクリスト',
  args: { html: taskListHtml },
  parameters: {
    docs: {
      description: {
        story:
          '壊れたら: checkbox 化が解けて素の [x] テキストに戻る、または contains-task-list / task-list-item class が剥がれて DOM が変わる。CSS サイドカーが無いと bullet が復活し、入れ子の字下げが消えると親子タスクが同じ横位置に潰れて絵が変わる (play で DOM と computed の両方を固定)。',
      },
    },
  },
  play: async ({ canvasElement }) => {
    await expect(canvasElement.querySelector('.contains-task-list')).not.toBeNull();
    await expect(canvasElement.querySelectorAll('.task-list-item')).toHaveLength(5);
    await expect(canvasElement.querySelectorAll('input[type="checkbox"]')).toHaveLength(5);
    // DOM 構造だけでは CSS 剥がれの平坦表示が「正常」として VRT baseline に焼き付く
    // (task PR#583)。CSS サイドカーの実効を computed で固定する:
    // 親 ul はあえて disc のまま、子 li だけ none にする。これにより子ルールを
    // 消すと継承した disc が復活するため、親ルール由来の none で空振りしない。
    // 親ルール削除は字下げ 0 の監視、子ルール削除は li の none の監視で落とす。
    const topList = canvasElement.querySelector(`.${KFM_CONTENT_CLASS} > ul.contains-task-list`);
    const nestedList = canvasElement.querySelector('.task-list-item > ul.contains-task-list');
    const taskItem = canvasElement.querySelector('.task-list-item');
    await expect(topList).not.toBeNull();
    await expect(nestedList).not.toBeNull();
    await expect(taskItem).not.toBeNull();
    const topStyle = topList ? getComputedStyle(topList) : null;
    const nestedStyle = nestedList ? getComputedStyle(nestedList) : null;
    await expect(topStyle?.listStyleType).toBe('disc');
    await expect(topStyle?.paddingLeft).toBe('0px');
    await expect(Number.parseFloat(nestedStyle?.paddingLeft ?? '0')).toBeGreaterThan(0);
    await expect(taskItem ? getComputedStyle(taskItem).listStyleType : '').toBe('none');
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
    const del = canvasElement.querySelector('del');
    await expect(del).not.toBeNull();
    // 打ち消し線の実効 (.kfm-content del)。del 要素があっても線が消えれば絵が変わる
    await expect(del ? getComputedStyle(del).textDecorationLine : '').toContain('line-through');
    const link = canvasElement.querySelector('a[href^="https://example.com"]');
    await expect(link).not.toBeNull();
    const style = link ? getComputedStyle(link) : null;
    await expect(style?.textDecoration).toContain('underline');
    // リンク色の実効 (#0969da)。旧検査 not.toBe('') は computed color が常に非空文字列の
    // ため何も検証していなかった (空振り)
    await expect(style?.color).toBe('rgb(9, 105, 218)');
  },
};

export const StrikeAutolinkDark: Story = {
  name: '打ち消し線・自動リンク（ダークテーマ）',
  render: kfmDarkRender,
  args: { html: strikeAutolinkHtml },
  parameters: {
    docs: {
      description: {
        story:
          '壊れたら: .dark 上書き (style.css の .dark .kfm-content a = #4493f8) が失われるとライト用の濃い青リンクがダーク地に沈み、絵が変わる。GFM サイドカーのダーク専用ルールはこの 1 本のみで、監視はこの story が担う。',
      },
    },
  },
  play: async ({ canvasElement }) => {
    // ダーク切替の実体 (.dark ancestor) が絵の前提として存在していること
    await expect(canvasElement.querySelector(`.dark .${KFM_CONTENT_CLASS}`)).not.toBeNull();
    const del = canvasElement.querySelector('del');
    await expect(del).not.toBeNull();
    await expect(del ? getComputedStyle(del).textDecorationLine : '').toContain('line-through');
    const link = canvasElement.querySelector('a[href^="https://example.com"]');
    await expect(link).not.toBeNull();
    const style = link ? getComputedStyle(link) : null;
    await expect(style?.textDecoration).toContain('underline');
    // ダークリンク色の実効 (#4493f8)。ライト色 rgb(9, 105, 218) のままなら .dark 上書きの喪失
    await expect(style?.color).toBe('rgb(68, 147, 248)');
  },
};

export const NestedLists: Story = {
  name: '入れ子のリスト',
  args: { html: nestedListsHtml },
  parameters: {
    docs: {
      description: {
        story:
          '壊れたら: 番号付き/記号リストの 5 段入れ子 (ol > ol > ul > ul > ul) が 1 段に潰れると play の DOM 構造が変わる。CSS サイドカーが無いとマーカー/インデントが消え絵も変わる。五段目は style.css の square ルールを実際に消費し、未使用 CSS を契約化するため残す。',
      },
    },
  },
  play: async ({ canvasElement }) => {
    await expect(canvasElement.querySelectorAll('ol ol ul ul ul')).toHaveLength(1);
    const outerOl = canvasElement.querySelector('ol');
    await expect(outerOl?.querySelectorAll(':scope > li')).toHaveLength(2);
    // マーカーとインデントの実効。CSS サイドカーが無いと preflight が両方消し、
    // DOM が入れ子のままでも絵は一段の平文に潰れる
    const outerStyle = outerOl ? getComputedStyle(outerOl) : null;
    await expect(outerStyle?.listStyleType).toBe('decimal');
    await expect(Number.parseFloat(outerStyle?.paddingLeft ?? '0')).toBeGreaterThan(0);
    const discUl = canvasElement.querySelector('ol ol ul');
    const circleUl = canvasElement.querySelector('ol ol ul ul');
    const deepUl = canvasElement.querySelector('ol ol ul ul ul');
    await expect(discUl ? getComputedStyle(discUl).listStyleType : '').toBe('disc');
    await expect(circleUl ? getComputedStyle(circleUl).listStyleType : '').toBe('circle');
    const deepStyle = deepUl ? getComputedStyle(deepUl) : null;
    // 五段目 ul は 3 重の ul なので、残すと決めた .kfm-content ul ul ul を実際に消費する
    await expect(deepStyle?.listStyleType).toBe('square');
    await expect(Number.parseFloat(deepStyle?.paddingLeft ?? '0')).toBeGreaterThan(0);
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
    const quotes = canvasElement.querySelectorAll('blockquote');
    await expect(quotes).toHaveLength(3);
    // 縦線 3 本の実効 (.kfm-content blockquote の border-left 0.25rem solid)。
    // 色はテーマトークン (--border) 追従のため幅と線種のみ固定する
    for (const quote of Array.from(quotes)) {
      const style = getComputedStyle(quote);
      await expect(style.borderLeftWidth).toBe('4px');
      await expect(style.borderLeftStyle).toBe('solid');
    }
  },
};

export const CodeFence: Story = {
  name: '着色済みコードフェンス（GFM 側の基準）',
  args: { html: codeFenceHtml },
  parameters: {
    docs: {
      description: {
        story:
          '壊れたら: starry-night 適用後も GFM のコードフェンスが pre/code 構造とエスケープを保ち、トークンが色分けされる基準。着色以外で絵が変わったらエスケープか pre/code 構造の変化。',
      },
    },
  },
};
