import { describe, expect, it } from 'vitest';
import { KFM_CODE_HIGHLIGHT_STORY_INPUTS } from '../kfm-story-fixtures/inputs-code-highlight';
import { renderDescription } from '../markup-renderer';

/**
 * KFM コードブロック着色 story fixture の drift 検査 (cmd_670 方式)。
 * stories/kfm/KfmCodeHighlight.stories.ts が v-html する HTML fixture
 * (kfm-story-fixtures/rendered/code-highlight-*.html) は renderDescription の
 * 事前生成物。レンダラ出力が変わったのに fixture が古いままだと VRT baseline が
 * 「本番と違う姿」を守り続けるため、現在出力との一致を CI で強制する
 * (CI では snapshot 新規作成も失敗 = fixture 追加漏れも落ちる)。
 * 再生成: pnpm test:unit --update
 */
describe('KFM code-highlight story fixtures (drift 検査)', () => {
  it.each(Object.entries(KFM_CODE_HIGHLIGHT_STORY_INPUTS))(
    'fixture %s は renderDescription の現在出力と一致する',
    async (name, input) => {
      await expect(await renderDescription(input)).toMatchFileSnapshot(
        `../kfm-story-fixtures/rendered/${name}.html`,
      );
    },
  );
});
