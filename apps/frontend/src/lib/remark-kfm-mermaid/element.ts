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
 * - 挿入前の最終防御 (parseSafeSvg) は sink と同じ HTML パースで検査し、href 系の
 *   実行可能スキームも明示的に拒否する。
 *   XML パーサを使わない理由はその関数のコメントを正とする。
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

/**
 * mermaid の返す SVG を shadow DOM へ入れる直前の構造の健全性検査。
 * URL は mermaid 側の sanitize-url に加え、ここでも実行可能スキームを拒否する。
 *
 * 検査は sink (shadow への挿入 = HTML fragment パース) と同じ HTML 文法で行う。
 * XML パーサ (image/svg+xml) を使わない決め手は二つ:
 * - mermaid の正常出力は XML 非適格になりうる。実測例: click 付き flowchart は
 *   ノードを `<a xlink:href=…>` で包むが `xmlns:xlink` 宣言を出力せず、XML では
 *   未宣言 prefix で parsererror、HTML では有効。XML 検査は sink が受け入れる
 *   正常出力を偽陽性で error に倒す。
 * - 逆方向も危険: XML として無害に見えても、HTML 再パースで構造が変わる入力
 *   (foreign content からの breakout = mXSS) は XML 検査を素通りする。
 *   検査と挿入の文法が違う限り、この双方向の食い違いは消えない。
 * ゆえに一度だけ HTML としてパースし、検査を通った「その同一ノード」を返して
 * 呼び出し側がそのまま挿入する (文字列の再パースをしない = 検査と挿入の乖離を断つ)。
 */
function parseSafeSvg(svg: string): DocumentFragment {
  const template = document.createElement('template');
  template.innerHTML = svg;
  const fragment = template.content;
  // 根が唯一の <svg> でなければ不正出力。HTML パースで要素が <svg> の外へ
  // 漏れた場合 (breakout) もここで childElementCount が 1 を超えて検出される
  if (fragment.childElementCount !== 1 || fragment.firstElementChild?.localName !== 'svg') {
    throw new Error('mermaid returned invalid SVG');
  }
  for (const element of fragment.querySelectorAll('*')) {
    if (element.localName === 'script') throw new Error('mermaid returned an unsafe SVG element');
    for (const attribute of element.attributes) {
      const name = attribute.name.toLowerCase();
      if (name.startsWith('on')) {
        throw new Error('mermaid returned an unsafe SVG attribute');
      }
      if (name === 'href' || name === 'xlink:href') {
        // HTML パースで実体参照は復号済み。URL parser が scheme 判定前に無視する
        // ASCII 制御・空白も除いてから判定し、java\nscript: 等の難読化を通さない。
        const normalized = attribute.value.replace(/[\u0000-\u0020\u007f-\u009f]/g, '');
        if (/^(?:javascript|vbscript|data):/i.test(normalized)) {
          throw new Error('mermaid returned an unsafe SVG URL');
        }
      }
    }
  }
  return fragment;
}

export function createKfmMermaidElement(): CustomElementConstructor {
  return class KfmMermaidElement extends HTMLElement {
    /** connectedCallback は移動・再接続でも複数回呼ばれる。描画は初回のみ */
    #started = false;
    /** 切断前に始まった非同期描画を、再接続後の要素へ反映させないための世代 */
    #renderToken = 0;

    connectedCallback(): void {
      if (this.#started) return;
      this.#started = true;
      void this.#render(++this.#renderToken);
    }

    disconnectedCallback(): void {
      if (this.dataset.kfmMermaid === undefined) {
        this.#started = false;
        this.#renderToken += 1;
      }
    }

    async #render(token: number): Promise<void> {
      // light DOM はサーバがエスケープしたソーステキストのみ (remark-kfm-mermaid 契約)。
      // textContent で実体参照は復元済みの生ソースが得られる。
      const source = this.textContent ?? '';
      let mermaid: Awaited<typeof import('mermaid')>['default'];
      try {
        ({ default: mermaid } = await import('mermaid'));
      } catch (error) {
        // chunk ロード失敗は一時的なネットワーク障害でありうる唯一の経路。
        // #started を戻し、再接続時の再試行だけを許す (構文エラー等の決定的失敗は
        // 再試行しても同じ結果ゆえ戻さない)。表示状態は rendered/error の二値のまま
        // (VRT / E2E の待ち口を増やさない)
        console.error('[kfm-mermaid] render failed', error);
        if (token !== this.#renderToken) return;
        this.#started = false;
        if (this.isConnected) {
          this.dataset.kfmMermaid = 'error' satisfies KfmMermaidState;
        }
        return;
      }
      try {
        // import 待ちの間に切断された場合はキューへ不要な仕事を積まず、再接続に委ねる。
        if (token !== this.#renderToken) return;
        if (!this.isConnected) {
          this.#started = false;
          return;
        }
        const { svg } = await enqueue(async () => {
          // アプリ本体・story と同じ .dark ancestor 方式でテーマを引く。
          // テーマは描画時スナップショット: 描画後に .dark が切り替わっても
          // 再描画・追従はしない (Phase 1 の割り切り。追従させる場合は .dark の
          // 変化を観測して再描画する口をここではなく consumer 側に足す)
          const dark = this.closest('.dark') !== null;
          mermaid.initialize({
            startOnLoad: false,
            securityLevel: 'strict',
            suppressErrorRendering: true,
            theme: dark ? 'dark' : 'default',
          });
          // 第三引数を省くと mermaid は一時描画ノードを document.body 末尾へ置くため、
          // 図が一瞬ページ末尾へ露出する。接続中の自要素内へ不可視コンテナを置き、
          // 成否にかかわらず撤去して light DOM のフォールバックを元どおり保つ。
          const renderContainer = document.createElement('div');
          renderContainer.setAttribute('aria-hidden', 'true');
          Object.assign(renderContainer.style, {
            position: 'fixed',
            visibility: 'hidden',
            pointerEvents: 'none',
            inset: '0 auto auto 0',
          });
          this.append(renderContainer);
          try {
            return await mermaid.render(
              `${KFM_MERMAID_TAG}-${renderSequence++}`,
              source,
              renderContainer,
            );
          } finally {
            renderContainer.remove();
          }
        });
        if (token !== this.#renderToken) return;
        if (!this.isConnected) {
          this.#started = false;
          return;
        }
        // 検査を通った同一ノードをそのまま挿入する (文字列を sink で再パースしない)
        const fragment = parseSafeSvg(svg);
        // shadow は成功時のみ張る: 失敗時に張ると light DOM のフォールバックまで隠れる
        const shadow = this.shadowRoot ?? this.attachShadow({ mode: 'open' });
        const style = document.createElement('style');
        style.textContent = ':host { display: block; }';
        shadow.replaceChildren(style, fragment);
        this.dataset.kfmMermaid = 'rendered' satisfies KfmMermaidState;
      } catch (error) {
        if (token !== this.#renderToken) return;
        console.error('[kfm-mermaid] render failed', error);
        if (!this.isConnected) {
          this.#started = false;
          return;
        }
        this.dataset.kfmMermaid = 'error' satisfies KfmMermaidState;
      }
    }
  };
}
