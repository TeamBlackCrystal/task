/**
 * element.ts — `<kfm-code>` custom element (client 専用・軽量シェル)。
 *
 * 仕事はコピーボタンを client 側で足すことだけ。SSR HTML には button を入れない
 * (sanitize の許可集合を button へ広げない。kfm-mermaid と同じ「SSR は不活性タグ、
 * 挙動は client で upgrade」の型)。帯・行番号・横スクロールは SSR 構造＋サイドカー
 * CSS が担い、JS 無効環境ではボタンが無いだけで表示は成立する。
 *
 * - constructor は factory で遅延 (_client-registry 契約: HTMLElement 不在の SSR / Node で
 *   モジュール評価が落ちない)。
 * - connectedCallback は移動・再接続でも複数回呼ばれる。ボタンは light DOM に残る
 *   (v-html の SSR HTML と同居) ため、既存ボタンの有無で冪等にする。
 * - コピーは code の textContent をそのまま writeText する。行番号は CSS の ::before
 *   (counter) で描かれ DOM テキストに存在しないため、コピーに混じらない (試験で固定)。
 * - clipboard が無い環境 (非 secure context 等) と writeText の reject は握り潰さず、
 *   ボタン表示 (x アイコン＋label) と console の両方で示す。
 *
 * 表示は文字ではなく lucide のアイコン (GitHub のコードブロックコピーボタンが手本):
 * 既定 copy → 成功 check → 失敗 x。custom element は Vue component を使えぬため、
 * repo が依存する @lucide/vue@1.31.0 と同じ path データで inline SVG を DOM 生成する
 * (下の ICON_NODES に出典を記す)。文字は aria-label / title にのみ残し、SVG は
 * aria-hidden="true" とする。
 *
 * VRT / E2E 向けの完了シグナル (時間待ち不要の口):
 * - 成功: 要素に `data-kfm-code-copy="copied"` が立ち、約 2 秒後に消える
 * - 失敗: `data-kfm-code-copy="failed"` が立ち、約 2 秒後に消える
 */
import { KFM_CODE_TAG } from './_tag';

export { KFM_CODE_TAG } from './_tag';

export type KfmCodeCopyState = 'copied' | 'failed';

/** client が足すコピーボタンの class (SSR には現れず sanitize 許可も不要) */
export const KFM_CODE_COPY_CLASS = 'kfm-code-copy';

const COPY_LABEL = 'コピー';
const COPIED_LABEL = 'コピーしました';
const FAILED_LABEL = 'コピーできませんでした';
/** 表示を戻すまでの時間 (GitHub のコピーボタンと同じ「一呼吸」) */
const RESET_DELAY_MS = 2000;

type IconName = 'copy' | 'check' | 'x';
type IconNode = ReadonlyArray<readonly [tag: string, attrs: Readonly<Record<string, string>>]>;

/**
 * 出典: @lucide/vue@1.31.0 (repo の既存依存) dist/cjs/lucide-vue.js の
 * __iconNode (copy / check / x) を key 属性を除いてそのまま写した。
 * lucide の既定描画 (viewBox 24・stroke currentColor・stroke-width 2・fill none・
 * linecap/linejoin round) は buildIconSvg 側で焼く。
 */
const ICON_NODES: Record<IconName, IconNode> = {
  copy: [
    ['rect', { width: '14', height: '14', x: '8', y: '8', rx: '2', ry: '2' }],
    ['path', { d: 'M4 16c-1.1 0-2-.9-2-2V4c0-1.1.9-2 2-2h10c1.1 0 2 .9 2 2' }],
  ],
  check: [['path', { d: 'M20 6 9 17l-5-5' }]],
  x: [
    ['path', { d: 'M18 6 6 18' }],
    ['path', { d: 'm6 6 12 12' }],
  ],
};

const SVG_NS = 'http://www.w3.org/2000/svg';

function buildIconSvg(name: IconName): SVGSVGElement {
  const svg = document.createElementNS(SVG_NS, 'svg');
  // lucide の既定に合わせる (寸法だけ 16 に落とす。viewBox は lucide の 24 のまま)
  svg.setAttribute('width', '16');
  svg.setAttribute('height', '16');
  svg.setAttribute('viewBox', '0 0 24 24');
  svg.setAttribute('fill', 'none');
  svg.setAttribute('stroke', 'currentColor');
  svg.setAttribute('stroke-width', '2');
  svg.setAttribute('stroke-linecap', 'round');
  svg.setAttribute('stroke-linejoin', 'round');
  // 意味は button の aria-label が担う。図は読み上げから隠す
  svg.setAttribute('aria-hidden', 'true');
  for (const [tag, attrs] of ICON_NODES[name]) {
    const child = document.createElementNS(SVG_NS, tag);
    for (const [attr, value] of Object.entries(attrs)) {
      child.setAttribute(attr, value);
    }
    svg.append(child);
  }
  return svg;
}

/** アイコンと文字 (aria-label / title) を一緒に切り替える (図と意味のずれを作らない) */
function applyButtonFace(button: HTMLButtonElement, icon: IconName, label: string): void {
  button.setAttribute('aria-label', label);
  button.title = label;
  button.replaceChildren(buildIconSvg(icon));
}

export function createKfmCodeElement(): CustomElementConstructor {
  return class KfmCodeElement extends HTMLElement {
    #resetTimer: ReturnType<typeof setTimeout> | undefined;
    #copyButton: HTMLButtonElement | undefined;

    connectedCallback(): void {
      // 再接続・HMR で二重にボタンを足さない (light DOM に残るため存在で判定)
      const existing = this.querySelector<HTMLButtonElement>(`:scope > .${KFM_CODE_COPY_CLASS}`);
      if (existing !== null) {
        this.#copyButton = existing;
        return;
      }
      const button = document.createElement('button');
      button.type = 'button';
      button.className = KFM_CODE_COPY_CLASS;
      applyButtonFace(button, 'copy', COPY_LABEL);
      button.addEventListener('click', () => {
        void this.#copy(button);
      });
      this.#copyButton = button;
      this.prepend(button);
    }

    disconnectedCallback(): void {
      // 切断後に発火した timer が別文書へ移った要素の表示を触らないようにする
      if (this.#resetTimer !== undefined) {
        clearTimeout(this.#resetTimer);
        this.#resetTimer = undefined;
      }
      const button = this.#copyButton;
      if (button !== undefined) {
        this.#resetCopyFace(button);
      }
    }

    #resetCopyFace(button: HTMLButtonElement): void {
      delete this.dataset.kfmCodeCopy;
      applyButtonFace(button, 'copy', COPY_LABEL);
    }

    async #copy(button: HTMLButtonElement): Promise<void> {
      // 行番号は CSS ::before で描かれ DOM テキストに無いため textContent が本文そのもの
      const source = this.querySelector('pre > code')?.textContent ?? '';
      let state: KfmCodeCopyState;
      try {
        if (navigator.clipboard === undefined) {
          throw new Error('clipboard API is unavailable');
        }
        await navigator.clipboard.writeText(source);
        state = 'copied';
      } catch (error) {
        // 失敗を握り潰さない: 表示 (x アイコン＋label＋状態属性) と console の両方で示す
        console.error('[kfm-code] copy failed', error);
        state = 'failed';
      }
      if (!this.isConnected) return;
      this.dataset.kfmCodeCopy = state;
      if (state === 'copied') {
        applyButtonFace(button, 'check', COPIED_LABEL);
      } else {
        applyButtonFace(button, 'x', FAILED_LABEL);
      }
      if (this.#resetTimer !== undefined) clearTimeout(this.#resetTimer);
      this.#resetTimer = setTimeout(() => {
        this.#resetTimer = undefined;
        this.#resetCopyFace(button);
      }, RESET_DELAY_MS);
    }
  };
}

// KFM_CODE_TAG を経由しない直書きを防ぐための再輸出 (kfm-mermaid/element.ts と同型)
void KFM_CODE_TAG;
