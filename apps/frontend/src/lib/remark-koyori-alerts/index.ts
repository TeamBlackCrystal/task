/**
 * remark-koyori-alerts — KFM 拡張第一号: GitHub alerts (`> [!NOTE]` 等) を callout へ変換する。
 *
 * GitHub 完全互換の境界仕様 (fixture テストで固定):
 * - 5 種のみ (NOTE / TIP / IMPORTANT / WARNING / CAUTION)・type は case-insensitive
 * - マーカーは blockquote 先頭行に単独。同一行に後続テキストがあれば通常 blockquote のまま
 *   (行末スペース 2 つの hard break は「単独」扱いで alert 化する)
 * - ネスト不可 (alert 内側の blockquote は通常 blockquote のまま)
 * - 5 種以外の type (`[!HINT]` 等) は通常 blockquote へフォールバック
 *
 * 出力は data.hName / hProperties の型付き hast のみ。生 html ノードと inline style は
 * 一切生成しない (sanitize 契約 FORBID_ATTR: ['style'] と一枚岩)。アイコンと配色は
 * style.css の名前空間クラス (.kfm-alert--* .kfm-alert__title::before) で当てる。
 */
import type { ElementContent, Properties } from 'hast';
import type { Paragraph, Root } from 'mdast';
import { SKIP, visit } from 'unist-util-visit';

// mdast-util-to-hast (remark-rehype 内部) と同一の Data 拡張。型付き hast emit の正規機構。
declare module 'mdast' {
  interface Data {
    hName?: string | undefined;
    hProperties?: Properties | undefined;
    hChildren?: ElementContent[] | undefined;
  }
}

const ALERT_TYPES = ['note', 'tip', 'important', 'warning', 'caution'] as const;
type AlertType = (typeof ALERT_TYPES)[number];

// マーカー行単独マッチ (行末までに非空白があれば不一致 = 同一行後続テキストのフォールバック)
const MARKER_RE = /^\[!(NOTE|TIP|IMPORTANT|WARNING|CAUTION)\][ \t]*$/i;

const TITLES: Record<AlertType, string> = {
  note: 'Note',
  tip: 'Tip',
  important: 'Important',
  warning: 'Warning',
  caution: 'Caution',
};

export function remarkKoyoriAlerts() {
  return (tree: Root): void => {
    visit(tree, 'blockquote', (node) => {
      const first = node.children[0];
      if (first?.type !== 'paragraph') return;
      const lead = first.children[0];
      if (lead?.type !== 'text') return;

      // remark-parse は blockquote 内の連続行を 1 つの text ノード (soft break = '\n') に
      // まとめるため、マーカー判定は先頭 text の 1 行目のみを見る。
      const newline = lead.value.indexOf('\n');
      const markerLine = newline === -1 ? lead.value : lead.value.slice(0, newline);
      const match = MARKER_RE.exec(markerLine);
      if (!match) return;
      // マーカーと同一行に inline 構文 (強調等) が続く場合も GitHub 準拠でフォールバック。
      // ただし行末スペース 2 つ (hard break) は独立した break ノードとしてここへ来るが、
      // GitHub はマーカー行単独として alert 化する。子の個数では inline 構文と hard break
      // を区別できないため「次の子が break か」で判定する。
      const second = first.children[1];
      if (newline === -1 && second !== undefined && second.type !== 'break') return;

      const type = match[1]!.toLowerCase() as AlertType;

      // マーカーを本文から除去
      if (newline === -1) {
        first.children.shift();
        // 行末スペース 2 つ由来の hard break はマーカーの一部として一緒に除去する
        // (残すと alert 本文の先頭に <br> が漏れる)
        if (first.children[0]?.type === 'break') first.children.shift();
        if (first.children.length === 0) node.children.shift();
      } else {
        lead.value = lead.value.slice(newline + 1);
      }

      node.data = {
        hName: 'div',
        hProperties: { className: ['kfm-alert', `kfm-alert--${type}`] },
      };
      const title: Paragraph = {
        type: 'paragraph',
        data: { hName: 'p', hProperties: { className: ['kfm-alert__title'] } },
        children: [{ type: 'text', value: TITLES[type] }],
      };
      node.children.unshift(title);

      // ネスト不可: alert 化した内側は走査しない (内側 blockquote は通常 blockquote のまま)
      return SKIP;
    });
  };
}

/**
 * 本プラグインが emit する class トークン (完全一致 allowlist)。
 * composition root が createRenderer({ sanitizeSchemas }) へ渡して sanitize registry と
 * 単一ソース化する。
 */
export const koyoriAlertsSanitizeSchema = {
  classTokens: [
    'kfm-alert',
    ...ALERT_TYPES.map((type) => `kfm-alert--${type}`),
    'kfm-alert__title',
  ],
} as const;
