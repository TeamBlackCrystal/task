import type { Meta, StoryObj } from '@storybook/vue3-vite';
import { expect } from 'storybook/test';
import allFiveHtml from '@/lib/kfm-story-fixtures/rendered/alerts-all-five.html?raw';
import hardBreakHtml from '@/lib/kfm-story-fixtures/rendered/alerts-hard-break-marker.html?raw';
import unknownTypeHtml from '@/lib/kfm-story-fixtures/rendered/alerts-unknown-type.html?raw';
// CSS サイドカー: レンダラは CSS を import しない契約のため、消費側 (= story) が明示 import。
// 本番消費側は alerts / GFM の両サイドカーを import するため story も両方揃える
// (片方だけだと UnknownType の素の blockquote 等が本番と違う絵で baseline になる)
import '@/lib/remark-koyori-alerts/style.css';
import '@/lib/remark-gfm/style.css';
// 器は本番と同じ .kfm-content (alerts CSS は .kfm-alert を直接指すため器 scope 不要だが、
// story の器 = 本番の器 という形を崩さない。単一ソース = content-class.ts)
import { KFM_CONTENT_CLASS } from '@/lib/remark-gfm/content-class';

/*
 * KFM GitHub alerts の story 群。
 * fixture は renderDescription の事前生成 HTML (単一ソース = kfm-story-fixtures/inputs.ts、
 * drift 検査 = kfm-story-fixtures.test.ts)。v-html のみの同期描画で VRT が決定的になる。
 */

type KfmStoryArgs = { html: string };

const ALERT_TYPES = ['note', 'tip', 'important', 'warning', 'caution'] as const;

// アクセント色 (縦線・タイトル) の期待値 = style.css の実色。CSS 変更でここが落ちるのは
// 意図した関門 (絵が変わる変更を play にも触らせる)。値は getComputedStyle の rgb 表記
const ALERT_ACCENTS: Record<(typeof ALERT_TYPES)[number], string> = {
  note: 'rgb(9, 105, 218)', // #0969da
  tip: 'rgb(26, 127, 55)', // #1a7f37
  important: 'rgb(130, 80, 223)', // #8250df
  warning: 'rgb(154, 103, 0)', // #9a6700
  caution: 'rgb(207, 34, 46)', // #cf222e
};

// .dark 上書き (GitHub ダークパレット) の期待値
const ALERT_ACCENTS_DARK: Record<(typeof ALERT_TYPES)[number], string> = {
  note: 'rgb(68, 147, 248)', // #4493f8
  tip: 'rgb(63, 185, 80)', // #3fb950
  important: 'rgb(171, 125, 248)', // #ab7df8
  warning: 'rgb(210, 153, 34)', // #d29922
  caution: 'rgb(248, 81, 73)', // #f85149
};

/** 5 種の callout に種別アクセント (縦線・タイトル色・アイコン) が実際に当たっていること */
const expectAlertAccents = async (
  canvasElement: HTMLElement,
  accents: Record<(typeof ALERT_TYPES)[number], string>,
) => {
  const maskImages: string[] = [];
  for (const type of ALERT_TYPES) {
    const alert = canvasElement.querySelector(`.kfm-alert--${type}`);
    await expect(alert).not.toBeNull();
    await expect(alert ? getComputedStyle(alert).borderLeftColor : '').toBe(accents[type]);
    const title = alert?.querySelector('.kfm-alert__title');
    await expect(title).not.toBeNull();
    await expect(title ? getComputedStyle(title).color : '').toBe(accents[type]);
    // アイコンの実体 (mask 塗りの ::before)。mask が剥がれると色付き矩形かアイコン無しになる。
    // 自作線画 SVG の data URI であることまで固定する (none や外部 URL への変化を弾く)
    const maskImage = (title ? getComputedStyle(title, '::before') : null)?.maskImage ?? 'none';
    await expect(maskImage).toContain('data:image/svg+xml');
    maskImages.push(maskImage);
  }
  // 5 種のアイコンが互いに異なること (変数取り違え・コピペで同じ絵に潰れる事故の検知)
  await expect(new Set(maskImages).size).toBe(ALERT_TYPES.length);
};

const kfmRender = (args: KfmStoryArgs) => ({
  setup: () => ({ args }),
  template: `<div class="${KFM_CONTENT_CLASS}" v-html="args.html" />`,
});

// アプリ本体と同じ .dark ancestor class 方式 (tailwind.css の @custom-variant dark)。
// 背景/文字色もアプリのテーマトークンで塗って実際のダーク画面と同じ地の上で撮る。
const kfmDarkRender = (args: KfmStoryArgs) => ({
  setup: () => ({ args }),
  template: `<div class="dark bg-background text-foreground p-4"><div class="${KFM_CONTENT_CLASS}" v-html="args.html" /></div>`,
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
          '壊れたら: 5 種の callout のどれかが blockquote に退化する・アクセント色/アイコンが消えると絵が変わる (プラグイン変換か sanitize の class 許可の変化。play で class と computed アクセントを固定)。',
      },
    },
  },
  play: async ({ canvasElement }) => {
    await expect(canvasElement.querySelectorAll('.kfm-alert')).toHaveLength(5);
    // class の存在だけでは「CSS サイドカー欠落で全部素の文になる」絵の変化を見逃す。
    // 種別アクセントの実効まで computed で固定する
    await expectAlertAccents(canvasElement, ALERT_ACCENTS);
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
          '壊れたら: .dark 上書き (style.css の GitHub ダークパレット) が失われるとライト用の濃色がダーク地に沈み、絵が変わる——種別アクセント色がライト実色のまま固定される退行の再発をこの絵で検知する。',
      },
    },
  },
  play: async ({ canvasElement }) => {
    await expect(canvasElement.querySelectorAll('.kfm-alert')).toHaveLength(5);
    // ダーク切替の実体 (.dark ancestor) が絵の前提として存在していること
    await expect(canvasElement.querySelector('.dark .kfm-alert')).not.toBeNull();
    // .dark 上書きの実効。種別アクセント色がライト実色のままダーク地に沈む退行を
    // ダークパレット実色の computed で固定する
    await expectAlertAccents(canvasElement, ALERT_ACCENTS_DARK);
  },
};

export const HardBreakMarker: Story = {
  name: 'マーカー行末スペース 2 つ（hard break 耐性の固定）',
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
          '壊れたら: [!HINT] が callout の絵になったら未知型フォールバックの崩れ (GitHub 互換の境界仕様違反)。素の blockquote の縦線 (GFM サイドカー) が消えても絵が変わる。',
      },
    },
  },
  play: async ({ canvasElement }) => {
    const quote = canvasElement.querySelector('blockquote');
    await expect(quote).not.toBeNull();
    await expect(canvasElement.querySelector('.kfm-alert')).toBeNull();
    // 「素の blockquote の絵」の実体 = GFM サイドカーの縦線 (本番器と同じ CSS 条件)
    const style = quote ? getComputedStyle(quote) : null;
    await expect(style?.borderLeftWidth).toBe('4px');
    await expect(style?.borderLeftStyle).toBe('solid');
  },
};
