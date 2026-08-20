import type { Meta, StoryObj } from '@storybook/vue3-vite';
import { expect, waitFor } from 'storybook/test';
import brokenHtml from '@/lib/kfm-story-fixtures/rendered/mermaid-broken.html?raw';
import clickHtml from '@/lib/kfm-story-fixtures/rendered/mermaid-click.html?raw';
import flowchartHtml from '@/lib/kfm-story-fixtures/rendered/mermaid-flowchart.html?raw';
import noneHtml from '@/lib/kfm-story-fixtures/rendered/mermaid-none.html?raw';
import sequenceHtml from '@/lib/kfm-story-fixtures/rendered/mermaid-sequence.html?raw';
import stateHtml from '@/lib/kfm-story-fixtures/rendered/mermaid-state.html?raw';
import '@/lib/remark-kfm-mermaid/style.css';
// 本番では +client.ts が行う client 登録を story でも同じ入口で行う (二重 define は registry 側で防止)
import { registerKfmCustomElements } from '@/lib/markup-renderer/_client-registry';

registerKfmCustomElements();

/*
 * KFM mermaid 図の story 群。fixture は renderDescription の事前生成 HTML だが、
 * 中身は SSR がブラウザへ渡す不活性 <kfm-mermaid> ＋ソーステキストのみで SVG を含まない
 * (mermaid は本質的にブラウザ描画のため、SVG まで事前 HTML 化する方式は採らない)。
 * 図の描画は story 上で custom element が実行し、play は data-kfm-mermaid の状態を待つ
 * (時間待ちなし)。この待ちが VRT の「描画完了してから撮る」口を兼ねる。
 */

type KfmStoryArgs = { html: string };

const kfmRender = (args: KfmStoryArgs) => ({
  setup: () => ({ args }),
  template: '<div class="kfm-story" v-html="args.html" />',
});

const meta = {
  title: 'KFM/Mermaid',
  tags: ['autodocs'],
  render: kfmRender,
} satisfies Meta<KfmStoryArgs>;

export default meta;
type Story = StoryObj<typeof meta>;

/** 描画完了 (成功) を状態で待ち、shadow DOM に SVG が実在することまで確かめる共通 assert */
async function expectRendered(root: ParentNode, selector = 'kfm-mermaid'): Promise<HTMLElement> {
  const element = root.querySelector<HTMLElement>(selector);
  await expect(element).not.toBeNull();
  await waitFor(
    () => expect(element?.dataset.kfmMermaid, 'data-kfm-mermaid が立つまで待つ').toBe('rendered'),
    { timeout: 10_000 },
  );
  await expect(element?.shadowRoot?.querySelector('svg')).not.toBeNull();
  return element as HTMLElement;
}

export const Flowchart: Story = {
  name: 'Flowchart（基本）',
  args: { html: flowchartHtml },
  parameters: {
    docs: {
      description: {
        story:
          '壊れたら: 図が出ずソーステキストのままなら client 登録か remark 層の変換の崩れ。data-kfm-mermaid が立たなければ描画完了シグナルの崩れ (VRT の待ち口が死ぬ)。',
      },
    },
  },
  play: async ({ canvasElement }) => {
    await expectRendered(canvasElement);
  },
};

export const Sequence: Story = {
  name: 'Sequence（シーケンス図）',
  args: { html: sequenceHtml },
  parameters: {
    docs: {
      description: {
        story: '壊れたら: participant/矢印の並びが崩れたら mermaid 更新か描画経路の崩れ。',
      },
    },
  },
  play: async ({ canvasElement }) => {
    await expectRendered(canvasElement);
  },
};

export const State: Story = {
  name: 'State（状態遷移図）',
  args: { html: stateHtml },
  parameters: {
    docs: {
      description: {
        story: '壊れたら: [*] 始端/終端や遷移ラベルが消えたら mermaid 更新か描画経路の崩れ。',
      },
    },
  },
  play: async ({ canvasElement }) => {
    await expectRendered(canvasElement);
  },
};

export const ClickLink: Story = {
  name: 'Click（リンク付き flowchart・正常出力の回帰）',
  args: { html: clickHtml },
  parameters: {
    docs: {
      description: {
        story:
          '壊れたら: click 付き flowchart は mermaid の正常出力であり rendered になるべき。error へ倒れたら、挿入前検査 (element.ts) が sink (shadow への HTML パース) と違う文法で正常出力を偽陽性にしている崩れ——実 mermaid 出力を通すこの story がその回帰アンカー。',
      },
    },
  },
  play: async ({ canvasElement }) => {
    await expectRendered(canvasElement);
  },
};

export const BrokenInput: Story = {
  name: '記法が壊れている入力（error 状態）',
  args: { html: brokenHtml },
  parameters: {
    docs: {
      description: {
        story:
          '壊れたら: エラー図や空白が出たら fail 側の契約 (shadow を張らずソーステキスト残置 ＋ data-kfm-mermaid="error") の崩れ。rendered になったら mermaid が構文エラーを黙って通した変化。',
      },
    },
  },
  play: async ({ canvasElement }) => {
    const element = canvasElement.querySelector<HTMLElement>('kfm-mermaid');
    await expect(element).not.toBeNull();
    await waitFor(
      () => expect(element?.dataset.kfmMermaid, 'error 状態へ倒れるまで待つ').toBe('error'),
      { timeout: 10_000 },
    );
    // フォールバック: shadow を張らず、ソーステキストがそのまま見える
    await expect(element?.shadowRoot).toBeNull();
    await expect(element?.textContent).toContain('閉じ括弧がない');
    await expect(getComputedStyle(element as HTMLElement).whiteSpace).toBe('pre');
    await expect(getComputedStyle(element as HTMLElement).overflowX).toBe('auto');
  },
};

export const DarkTheme: Story = {
  name: 'ダークテーマ（ライトと並置）',
  args: { html: flowchartHtml },
  // 同一 fixture をライト地とダーク地 (.dark ancestor = アプリ本体と同方式) で並置
  render: (args: KfmStoryArgs) => ({
    setup: () => ({ args }),
    template:
      '<div class="grid gap-4">' +
      '<div class="p-4"><div class="kfm-story kfm-story-light" v-html="args.html" /></div>' +
      '<div class="dark bg-background text-foreground p-4"><div class="kfm-story kfm-story-dark" v-html="args.html" /></div>' +
      '</div>',
  }),
  parameters: {
    docs: {
      description: {
        story:
          '壊れたら: 上下の箱で図の配色が同じになったら .dark ancestor からのテーマ切替 (element.ts) の喪失——alerts/着色の「ライト固定」と同型の再発をこの絵で検知する。',
      },
    },
  },
  play: async ({ canvasElement }) => {
    const light = await expectRendered(canvasElement, '.kfm-story-light kfm-mermaid');
    const dark = await expectRendered(canvasElement, '.kfm-story-dark kfm-mermaid');
    // 同一ソースでもテーマが違えば SVG (テーマ変数を焼き込んだ style を含む) は一致しない。
    // パレット実値はハードコードせず相対比較 (mermaid 更新に強い)
    await expect(light.shadowRoot?.innerHTML).not.toBe(dark.shadowRoot?.innerHTML);
  },
};

export const NoDiagram: Story = {
  name: '図を含まないページ（何も起きない）',
  args: { html: noneHtml },
  parameters: {
    docs: {
      description: {
        story:
          '壊れたら: 図なしページに kfm-mermaid が生まれたら remark 層の捕捉範囲の崩れ。ts フェンスが pre>code でなくなったら他フェンスへの越境。',
      },
    },
  },
  play: async ({ canvasElement }) => {
    await expect(canvasElement.querySelector('kfm-mermaid')).toBeNull();
    // 陽性対照: 通常のコードフェンスは無傷で pre>code のまま
    await expect(canvasElement.querySelector('pre code')).not.toBeNull();
  },
};
