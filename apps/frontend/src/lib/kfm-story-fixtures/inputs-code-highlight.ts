/**
 * KFM コードブロック着色 story fixture の入力 (単一ソース)。
 * cmd_670 (feat/kfm-stories) の fixture+v-html 方式に揃えた同型実装。
 * 670 側の inputs.ts / rendered/*.html とはファイル・fixture 名を重ねず
 * (code-highlight- 接頭辞)、両枝が親 feat/kfm-phase1 へどの順で merge しても
 * 衝突しないようにしている。
 *
 * drift 検査: src/lib/__tests__/kfm-code-highlight-fixtures.test.ts の
 * toMatchFileSnapshot が担う。レンダラ出力が変わったのに fixture が古いままだと
 * CI (test:unit) が落ちる。再生成: pnpm test:unit --update
 */
export const KFM_CODE_HIGHLIGHT_STORY_INPUTS = {
  // クラスの出方が違う言語を三種: ts (型注釈/テンプレート literal)、
  // rust (マクロ/ライフタイム風), python (def/文字列/コメント)
  'code-highlight-typescript': [
    '```ts',
    'const total: number = items.length; // コメント',
    'function greet(name: string): string {',
    '  return `hello ${name}`;',
    '}',
    '```',
  ].join('\n'),

  'code-highlight-rust': [
    '```rust',
    'fn main() {',
    '    let x: u32 = 1;',
    '    println!("{} 件", x); // マクロ',
    '}',
    '```',
  ].join('\n'),

  'code-highlight-python': [
    '```python',
    'def total(items):',
    '    """docstring"""',
    '    return f"{len(items)} 件"  # コメント',
    '```',
  ].join('\n'),

  // 言語指定なし: language-* class が付かず、着色もされない素の姿
  'code-highlight-no-language': [
    '```',
    'plain fence: const x = 1; <b>タグも素通しではなくテキスト</b>',
    '```',
  ].join('\n'),

  // 未知言語: 落ちずに素のコードブロックへフォールバックし、内容はエスケープ維持
  'code-highlight-unknown-language': [
    '```definitelynotalang',
    '<b>&amp; エスケープされたまま</b>',
    '```',
  ].join('\n'),

  // 横に長い 1 行 (横溢れの見え方)
  'code-highlight-long-line': [
    '```ts',
    'const result = veryLongFunctionName(firstArgument, secondArgument, thirdArgument).then((response) => transformResponsePayload(response, { includeMetadata: true, normalizeWhitespace: true })).catch(handleUnexpectedRenderingFailure);',
    '```',
  ].join('\n'),
} as const;

export type KfmCodeHighlightStoryFixtureName = keyof typeof KFM_CODE_HIGHLIGHT_STORY_INPUTS;
