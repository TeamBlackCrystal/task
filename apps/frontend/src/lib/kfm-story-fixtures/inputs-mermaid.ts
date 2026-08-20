/**
 * KFM mermaid story fixture の入力 (単一ソース)。
 * fixture (rendered/mermaid-*.html) は renderDescription の事前生成 HTML だが、
 * 中身は「不活性 <kfm-mermaid> ＋ light DOM ソーステキスト」= SSR がブラウザへ渡す姿
 * そのものであり、SVG は含まない (図の描画は story 上で custom element が client 実行する。
 * SVG まで事前 HTML 化する fixture+v-html 方式は mermaid では不可)。
 *
 * 他枝の inputs*.ts と fixture 名を重ねない (mermaid- 接頭辞)。
 * drift 検査: src/lib/__tests__/kfm-mermaid-fixtures.test.ts (再生成: pnpm test:unit --update)
 */
export const KFM_MERMAID_STORY_INPUTS = {
  // 代表三種: flowchart / sequence / state (図種ごとに mermaid 内部の描画経路が異なる)
  'mermaid-flowchart': [
    '```mermaid',
    'flowchart TD',
    '  A[入力] --> B{分岐}',
    '  B -->|はい| C[処理]',
    '  B -->|いいえ| D[終了]',
    '```',
  ].join('\n'),

  'mermaid-sequence': [
    '```mermaid',
    'sequenceDiagram',
    '  participant C as Client',
    '  participant S as Server',
    '  C->>S: リクエスト',
    '  S-->>C: レスポンス',
    '```',
  ].join('\n'),

  'mermaid-state': [
    '```mermaid',
    'stateDiagram-v2',
    '  [*] --> Idle',
    '  Idle --> Running: start',
    '  Running --> Idle: stop',
    '  Running --> [*]',
    '```',
  ].join('\n'),

  // click 付き flowchart: mermaid の正常出力が XML 非適格になりうる代表例。
  // 検査 (element.ts) が sink と同じ HTML パースであることの回帰アンカー
  'mermaid-click': [
    '```mermaid',
    'flowchart TD',
    '  A[リンク付きノード] --> B[通常ノード]',
    '  click A "https://example.com"',
    '```',
  ].join('\n'),

  // 記法が壊れている入力: client 描画が error 状態へ倒れ、ソーステキストが残る
  'mermaid-broken': ['```mermaid', 'flowchart TD', '  A[閉じ括弧がない --> B', '```'].join('\n'),

  // 図を含まないページ: <kfm-mermaid> が生まれず、通常のコードフェンスは無傷
  'mermaid-none': [
    '段落テキストのみのページ。',
    '',
    '```ts',
    'const untouched = true;',
    '```',
  ].join('\n'),
} as const;

export type KfmMermaidStoryFixtureName = keyof typeof KFM_MERMAID_STORY_INPUTS;
