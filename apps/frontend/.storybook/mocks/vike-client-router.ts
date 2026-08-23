export async function navigate(url: string) {
  window.history.pushState({}, '', url);
  window.dispatchEvent(new PopStateEvent('popstate'));
}

// vike/client/router の reload。実体はサーバの +data.ts を再実行して
// pageContext を差し替えるが、Storybook には Vike のランタイムが無いので何もしない。
// 消費側 (タスク詳細の説明保存後の描画取り直し) は「呼べること」だけを要求する。
// この mock は実モジュールの影である。+Page.vue が import する export を増やしたら
// ここにも足すこと — 足し忘れると Storybook のビルドが MISSING_EXPORT で落ちる。
export async function reload() {}
