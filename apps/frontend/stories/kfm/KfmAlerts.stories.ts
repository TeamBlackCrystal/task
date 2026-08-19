import type { Meta, StoryObj } from '@storybook/vue3-vite';
import { expect } from 'storybook/test';
import allFiveHtml from '@/lib/kfm-story-fixtures/rendered/alerts-all-five.html?raw';
import hardBreakHtml from '@/lib/kfm-story-fixtures/rendered/alerts-hard-break-marker.html?raw';
import unknownTypeHtml from '@/lib/kfm-story-fixtures/rendered/alerts-unknown-type.html?raw';
// CSS サイドカー: レンダラは CSS を import しない契約のため、消費側 (= story) が明示 import
import '@/lib/remark-koyori-alerts/style.css';

/*
 * KFM GitHub alerts の story 群。
 * fixture は renderDescription の事前生成 HTML (単一ソース = kfm-story-fixtures/inputs.ts、
 * drift 検査 = kfm-story-fixtures.test.ts)。v-html のみの同期描画で VRT が決定的になる。
 */

type KfmStoryArgs = { html: string };

const ALERT_TYPES = ['note', 'tip', 'important', 'warning', 'caution'] as const;

const kfmRender = (args: KfmStoryArgs) => ({
  setup: () => ({ args }),
  template: '<div class="kfm-story" v-html="args.html" />',
});

// アプリ本体と同じ .dark ancestor class 方式 (tailwind.css の @custom-variant dark)。
// 背景/文字色もアプリのテーマトークンで塗って実際のダーク画面と同じ地の上で撮る。
const kfmDarkRender = (args: KfmStoryArgs) => ({
  setup: () => ({ args }),
  template:
    '<div class="dark bg-background text-foreground p-4"><div class="kfm-story" v-html="args.html" /></div>',
});

const meta = {
  title: 'KFM/Alerts',
  tags: ['autodocs'],
  render: kfmRender,
} satisfies Meta<KfmStoryArgs>;

export default meta;
type Story = StoryObj<typeof meta>;

export const AllFive: Story = {
  name: '5 種すべて（ライト）',
  args: { html: allFiveHtml },
  parameters: {
    docs: {
      description: {
        story:
          '壊れたら: 5 種の callout のどれかが blockquote に退化する・アクセント色/アイコンが消えると絵が変わる (プラグイン変換か sanitize の class 許可の変化)。',
      },
    },
  },
  play: async ({ canvasElement }) => {
    await expect(canvasElement.querySelectorAll('.kfm-alert')).toHaveLength(5);
    for (const type of ALERT_TYPES) {
      await expect(canvasElement.querySelector(`.kfm-alert--${type}`)).not.toBeNull();
    }
  },
};

export const AllFiveDark: Story = {
  name: '5 種すべて（ダークテーマ）',
  render: kfmDarkRender,
  args: { html: allFiveHtml },
  parameters: {
    docs: {
      description: {
        story:
          '壊れたら: .dark 上書き (style.css の GitHub ダークパレット) が失われるとライト用の濃色がダーク地に沈み、絵が変わる——yupix 殿指摘 4 件目 (CSS ライト固定) の再発をこの絵で検知する。',
      },
    },
  },
  play: async ({ canvasElement }) => {
    await expect(canvasElement.querySelectorAll('.kfm-alert')).toHaveLength(5);
    // ダーク切替の実体 (.dark ancestor) が絵の前提として存在していること
    await expect(canvasElement.querySelector('.dark .kfm-alert')).not.toBeNull();
  },
};

export const HardBreakMarker: Story = {
  name: 'マーカー行末スペース 2 つ（cmd_668 修正の固定）',
  args: { html: hardBreakHtml },
  parameters: {
    docs: {
      description: {
        story:
          '壊れたら: 行末スペース 2 つ付きマーカーが alert 化されず blockquote に戻る、または本文先頭に空行 (漏れた br) が現れると絵が変わる。',
      },
    },
  },
  play: async ({ canvasElement }) => {
    const alert = canvasElement.querySelector('.kfm-alert--warning');
    await expect(alert).not.toBeNull();
    await expect(alert?.innerHTML).not.toContain('<br');
  },
};

export const UnknownType: Story = {
  name: '未知の型（[!HINT]）は素の blockquote',
  args: { html: unknownTypeHtml },
  parameters: {
    docs: {
      description: {
        story:
          '壊れたら: [!HINT] が callout の絵になったら未知型フォールバックの崩れ (GitHub 互換の境界仕様違反)。',
      },
    },
  },
  play: async ({ canvasElement }) => {
    await expect(canvasElement.querySelector('blockquote')).not.toBeNull();
    await expect(canvasElement.querySelector('.kfm-alert')).toBeNull();
  },
};
