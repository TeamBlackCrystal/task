/**
 * element.ts — `<kfm-code>` custom element (client 専用・軽量シェル)。
 *
 * 仕事は複写ボタンを client 側で足すことだけ。SSR HTML には button を入れない
 * (sanitize の許可集合を button へ広げない。kfm-mermaid と同じ「SSR は不活性タグ、
 * 挙動は client で upgrade」の型)。帯・行番号・横スクロールは SSR 構造＋サイドカー
 * CSS が担い、JS 無効環境でもボタンが無いだけで表示は成立する。
 *
 * - constructor は factory で遅延 (_client-registry 契約: HTMLElement 不在の SSR / Node で
 *   モジュール評価が落ちない)。
 * - connectedCallback は移動・再接続でも複数回呼ばれる。ボタンは light DOM に残る
 *   (v-html の SSR HTML と同居) ため、既存ボタンの有無で冪等にする。
 * - 複写は code の textContent をそのまま writeText する。行番号は CSS の ::before
 *   (counter) で描かれ DOM テキストに存在しないため、複写に混じらない (試験で固定)。
 * - clipboard が無い環境 (非 secure context 等) と writeText の reject は握り潰さず、
 *   ボタン表示と data-kfm-code-copy="failed" で示す。
 *
 * VRT / E2E 向けの完了シグナル (時間待ち不要の口):
 * - 成功: 要素に `data-kfm-code-copy="copied"` が立ち、約 2 秒後に消える
 * - 失敗: `data-kfm-code-copy="failed"` が立ち、約 2 秒後に消える
 */
import { KFM_CODE_TAG } from './_tag';

export { KFM_CODE_TAG } from './_tag';

export type KfmCodeCopyState = 'copied' | 'failed';

/** client が足す複写ボタンの class (SSR には現れず sanitize 許可も不要) */
export const KFM_CODE_COPY_CLASS = 'kfm-code-copy';

const COPY_LABEL = '複写';
const COPIED_LABEL = '複写した';
const FAILED_LABEL = '複写できず';
/** 表示を戻すまでの時間 (GitHub の複写ボタンと同じ「一呼吸」) */
const RESET_DELAY_MS = 2000;

export function createKfmCodeElement(): CustomElementConstructor {
  return class KfmCodeElement extends HTMLElement {
    #resetTimer: ReturnType<typeof setTimeout> | undefined;

    connectedCallback(): void {
      // 再接続・HMR で二重にボタンを足さない (light DOM に残るため存在で判定)
      if (this.querySelector(`:scope > .${KFM_CODE_COPY_CLASS}`) !== null) return;
      const button = document.createElement('button');
      button.type = 'button';
      button.className = KFM_CODE_COPY_CLASS;
      button.textContent = COPY_LABEL;
      button.setAttribute('aria-label', 'コードを複写');
      button.addEventListener('click', () => {
        void this.#copy(button);
      });
      this.prepend(button);
    }

    disconnectedCallback(): void {
      // 切断後に発火した timer が別文書へ移った要素の表示を触らないようにする
      if (this.#resetTimer !== undefined) {
        clearTimeout(this.#resetTimer);
        this.#resetTimer = undefined;
      }
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
        // 失敗を握り潰さない: 表示 (ボタン文言＋状態属性) と console の両方で示す
        console.error('[kfm-code] copy failed', error);
        state = 'failed';
      }
      if (!this.isConnected) return;
      this.dataset.kfmCodeCopy = state;
      button.textContent = state === 'copied' ? COPIED_LABEL : FAILED_LABEL;
      if (this.#resetTimer !== undefined) clearTimeout(this.#resetTimer);
      this.#resetTimer = setTimeout(() => {
        this.#resetTimer = undefined;
        delete this.dataset.kfmCodeCopy;
        button.textContent = COPY_LABEL;
      }, RESET_DELAY_MS);
    }
  };
}

// KFM_CODE_TAG を経由しない直書きを防ぐための再輸出 (kfm-mermaid/element.ts と同型)
void KFM_CODE_TAG;
