/**
 * element.ts — `<kfm-mermaid>` custom element (client 専用・遅延ロード)。
 *
 * - mermaid 本体は connectedCallback 内の dynamic import のみで参照する。静的 import に
 *   すると +client.ts → _client-registry → 本ファイルの経路で mermaid 一式が client の
 *   同期バンドルへ載る (KFM 一式 +417.5 KB の前例と同型)。本ファイル自体は登録 seam に
 *   静的 import される前提の軽量シェルであり、重量物は図が実在するページでだけ落ちてくる。
 * - constructor は factory で遅延 (_client-registry 契約: HTMLElement 不在の SSR / Node で
 *   モジュール評価が落ちない)。
 * - securityLevel は strict 固定 (緩める変更は禁止)。suppressErrorRendering で
 *   構文エラー時に mermaid が DOM へエラー図を注入するのも止め、失敗は状態値で表す。
 *
 * VRT / E2E 向けの完了シグナル (時間待ち不要の口):
 * - 成功: `data-kfm-mermaid="rendered"` が立ち、shadow DOM に SVG が入る
 * - 失敗 (構文エラー・ロード失敗): `data-kfm-mermaid="error"` が立ち、shadow を張らず
 *   light DOM のソーステキストがフォールバック表示のまま残る
 * - 属性なし: 未処理 (ロード中 or JS 無効)。待つ側は属性の出現を監視すればよい。
 *
 * mermaid.initialize はモジュール singleton の全体設定であり、テーマ (light/dark) は
 * 要素ごとに異なりうる。initialize→render の組を module 級キューで直列化し、
 * 並行 connect 時に他要素の initialize が進行中 render の設定を書き換える競争を断つ。
 */
import { KFM_MERMAID_TAG } from './_tag';

export { KFM_MERMAID_TAG } from './_tag';

export type KfmMermaidState = 'rendered' | 'error';

/** mermaid.render の要求する一意 id 用 (DOM id は描画後の SVG にのみ残る) */
let renderSequence = 0;

/** initialize→render を要素間で直列化するキュー (失敗しても鎖は切らない) */
let renderQueue: Promise<unknown> = Promise.resolve();

function enqueue<T>(task: () => Promise<T>): Promise<T> {
  const run = renderQueue.then(task, task);
  renderQueue = run.then(
    () => undefined,
    () => undefined,
  );
  return run;
}

export function createKfmMermaidElement(): CustomElementConstructor {
  return class KfmMermaidElement extends HTMLElement {
    /** connectedCallback は移動・再接続でも複数回呼ばれる。描画は初回のみ */
    #started = false;

    connectedCallback(): void {
      if (this.#started) return;
      this.#started = true;
      void this.#render();
    }

    async #render(): Promise<void> {
      // light DOM はサーバがエスケープしたソーステキストのみ (remark-kfm-mermaid 契約)。
      // textContent で実体参照は復元済みの生ソースが得られる。
      const source = this.textContent ?? '';
      try {
        const { default: mermaid } = await import('mermaid');
        const { svg } = await enqueue(() => {
          // アプリ本体・story と同じ .dark ancestor 方式でテーマを引く
          const dark = this.closest('.dark') !== null;
          mermaid.initialize({
            startOnLoad: false,
            securityLevel: 'strict',
            suppressErrorRendering: true,
            theme: dark ? 'dark' : 'default',
          });
          return mermaid.render(`${KFM_MERMAID_TAG}-${renderSequence++}`, source);
        });
        if (!this.isConnected) return;
        // shadow は成功時のみ張る: 失敗時に張ると light DOM のフォールバックまで隠れる
        const shadow = this.shadowRoot ?? this.attachShadow({ mode: 'open' });
        shadow.innerHTML = `<style>:host { display: block; }</style>${svg}`;
        this.dataset.kfmMermaid = 'rendered' satisfies KfmMermaidState;
      } catch {
        if (!this.isConnected) return;
        this.dataset.kfmMermaid = 'error' satisfies KfmMermaidState;
      }
    }
  };
}
