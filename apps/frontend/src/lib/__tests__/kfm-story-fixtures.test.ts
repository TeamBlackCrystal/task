import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { describe, expect, it } from 'vitest';
import { KFM_MERMAID_STORY_INPUTS } from '../kfm-story-fixtures/inputs-mermaid';
import { KFM_STORY_INPUTS } from '../kfm-story-fixtures/inputs';
import { renderDescription } from '../markup-renderer';

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const RENDERED_DIR = path.join(__dirname, '../kfm-story-fixtures/rendered');

/**
 * KFM story fixture の drift 検査。
 *
 * stories/kfm/* が v-html する HTML fixture (kfm-story-fixtures/rendered/*.html) は
 * renderDescription の事前生成物。レンダラの出力が変わったのに fixture が古いままだと、
 * VRT baseline が「本番と違う姿」を守り続ける。ここで現在出力と committed fixture の
 * 一致を CI で強制する (CI では snapshot の新規作成も失敗になる = fixture 追加漏れも落ちる)。
 * 再生成: pnpm test:unit --update
 */
describe('KFM story fixtures (drift 検査)', () => {
  it.each(Object.entries(KFM_STORY_INPUTS))(
    'fixture %s は renderDescription の現在出力と一致する',
    async (name, input) => {
      await expect(await renderDescription(input)).toMatchFileSnapshot(
        `../kfm-story-fixtures/rendered/${name}.html`,
      );
    },
  );

  // この検査だけは rendered/ ディレクトリ全体を見る。drift 検査 (上の it.each) が
  // 層ごとのファイルに分かれているのに対し、孤立 fixture は層を跨いで一箇所に溜まるため。
  // ゆえに **fixture を持つ層を足したら、その入力一覧をここへ足すこと**。
  // 足し忘れると、新しい層の fixture が丸ごと「孤立」と誤判定されて落ちる。
  const ALL_STORY_INPUTS = {
    ...KFM_STORY_INPUTS,
      ...KFM_MERMAID_STORY_INPUTS,
  };

  it('rendered/*.html に対応する入力キーが無い孤立 fixture が無い', () => {
    const htmlFiles = fs
      .readdirSync(RENDERED_DIR)
      .filter((file) => file.endsWith('.html'))
      .sort();
    const inputKeys = new Set(Object.keys(ALL_STORY_INPUTS).map((name) => `${name}.html`));
    const orphans = htmlFiles.filter((file) => !inputKeys.has(file));
    expect(orphans).toEqual([]);
  });
});
