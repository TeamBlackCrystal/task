/**
 * KFM story fixture の入力 (単一ソース)。
 *
 * 契約: stories/kfm/* が v-html する HTML fixture (rendered/*.html) は、この入力を
 * renderDescription へ通した事前生成物である (本番も「サーバ生成 HTML を v-html する
 * だけ」なので story は本番と同じ姿・同期描画になり、VRT が決定的になる)。
 *
 * drift 検査: 生成と検証は src/lib/__tests__/kfm-story-fixtures.test.ts の
 * toMatchFileSnapshot が担う。レンダラの出力が変わったのに fixture が古いままだと
 * CI (test:unit) が落ちる。「更新を忘れない」運用ではなく機械で強制する。
 * 再生成: pnpm test:unit --update
 *
 * story は「壊れたら気づける単位」で切る (キー 1 つ = fixture 1 枚 = story 1 つが基本)。
 */

// 行末スペース 2 つ (hard break) を editor / formatter の trailing-whitespace 除去から
// 守るため、埋め込みではなく明示の定数で連結する。
const TWO_TRAILING_SPACES = '  ';

export const KFM_STORY_INPUTS = {
  'gfm-table-alignment': [
    '| 左揃え | 中央揃え | 右揃え |',
    '| :-- | :-: | --: |',
    '| あ | い | 100 |',
    '| 長めのセル内容 | 中央 | 2,400 |',
  ].join('\n'),

  'gfm-table-overflow': [
    '| 列1 | 列2 | 列3 | 列4 | 列5 | 列6 | 列7 | 列8 |',
    '| --- | --- | --- | --- | --- | --- | --- | --- |',
    '| 横に長いセルの内容その一 | 横に長いセルの内容その二 | 横に長いセルの内容その三 | 横に長いセルの内容その四 | 横に長いセルの内容その五 | 横に長いセルの内容その六 | 横に長いセルの内容その七 | 横に長いセルの内容その八 |',
  ].join('\n'),

  'gfm-task-list': [
    '- [x] 済みタスク',
    '- [ ] 未了タスク',
    '- [ ] 親タスク',
    '  - [x] 子タスク (ネスト)',
    '  - [ ] 子タスク 2',
  ].join('\n'),

  'gfm-strike-autolink': [
    '~~取り消された文~~ と残る文。',
    '',
    '本文 https://example.com/very/long/path?query=1 を参照。',
  ].join('\n'),

  'gfm-nested-lists': [
    '1. 番号付き一段目',
    '   1. 番号付き二段目',
    '      - 記号三段目',
    '        - 記号四段目',
    '          - 記号五段目 (square マーカー)',
    '2. 番号付き一段目に戻る',
    '   - 記号二段目',
  ].join('\n'),

  'gfm-deep-quote': ['> 一段目の引用', '>', '> > 二段目の引用', '> >', '> > > 三段目の引用'].join(
    '\n',
  ),

  // cmd_669 (starry-night 着色) の前後で差が見える基準。着色が入ると drift 検査が落ち、
  // 再生成後の絵に pl- span が現れる。
  'gfm-code-fence': [
    '```js',
    "const message = 'GFM fence baseline';",
    'console.log(message);',
    '```',
  ].join('\n'),

  // コード着色 story。クラスの出方が違う言語を三種と、非着色境界・横溢れを固定する。
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

  'code-highlight-no-language': [
    '```',
    'plain fence: const x = 1; <b>タグも素通しではなくテキスト</b>',
    '```',
  ].join('\n'),

  'code-highlight-unknown-language': [
    '```definitelynotalang',
    '<b>&amp; エスケープされたまま</b>',
    '```',
  ].join('\n'),

  'code-highlight-long-line': [
    '```ts',
    'const result = veryLongFunctionName(firstArgument, secondArgument, thirdArgument).then((response) => transformResponsePayload(response, { includeMetadata: true, normalizeWhitespace: true })).catch(handleUnexpectedRenderingFailure);',
    '```',
  ].join('\n'),

  'alerts-all-five': [
    '> [!NOTE]',
    '> 補足情報の callout。',
    '',
    '> [!TIP]',
    '> 助言の callout。',
    '',
    '> [!IMPORTANT]',
    '> 重要事項の callout。',
    '',
    '> [!WARNING]',
    '> 警告の callout。',
    '',
    '> [!CAUTION]',
    '> 危険の callout。',
  ].join('\n'),

  // cmd_668 で直した箇所: マーカー行末に半角スペース 2 つ (hard break) があっても
  // alert 化し、本文先頭へ <br> が漏れない。直った状態を絵で固定する。
  'alerts-hard-break-marker': [
    `> [!WARNING]${TWO_TRAILING_SPACES}`,
    '> 行末スペース 2 つ付きマーカーの本文。',
  ].join('\n'),

  'alerts-unknown-type': ['> [!HINT]', '> 未知の型は素の blockquote のまま。'].join('\n'),

  // 通してはならぬもの (生 HTML の script) と、通すべきもの (フェンス内の同じ文字列が
  // エスケープ済みテキストとして見えること) を同じ絵に並べる。
  'sanitize-script': [
    '生 HTML の script はこの下から消える:',
    '',
    '<script>alert(1)</script>',
    '',
    'フェンス内の同じ文字列は無害なテキストとして残る:',
    '',
    '```html',
    '<script>alert(1)</script>',
    '```',
  ].join('\n'),

  // inline style 付き生 HTML の落ち方は要素で違う: block の div は要素ごと消えるが、
  // inline の span はタグだけ落ちて中のテキストが素の文として残る (rendered fixture 参照)。
  // どちらも style は絵に現れない。markdown の強調は通る。
  'sanitize-inline-style': [
    '**強調は通る**。次の行の赤字指定 (inline style) は絵に現れない:',
    '',
    '<span style="color:red">style 付き生 HTML</span>',
    '',
    '<div style="position:fixed;inset:0">画面を覆う style 付き生 HTML</div>',
  ].join('\n'),

  // アプリ側クラスの騙り: 生 HTML で kfm-alert を名乗っても alert の絵は増えない。
  // 正規の alert (プラグイン経由) だけが callout になる。
  // タスク詳細ページ story (Pages/TaskDetail の DescriptionKfmRendered) の入力対。
  // story は本番同様 descriptionHtml/descriptionSource の対で受けるため、
  // descriptionSource 側もこの入力 (単一ソース) から取ること。
  'task-detail-description': '**強調** と `code` を含む説明',

  'sanitize-class-spoof': [
    '> [!NOTE]',
    '> これは正規の alert (絵に出る callout はこれ 1 つ)。',
    '',
    '<div class="kfm-alert kfm-alert--caution">偽 alert (生 HTML) — callout にならない</div>',
  ].join('\n'),
} as const;

export type KfmStoryFixtureName = keyof typeof KFM_STORY_INPUTS;
