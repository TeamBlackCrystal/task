import { describe, expect, it } from 'vitest';
import { KFM_STORY_INPUTS } from '../kfm-story-fixtures/inputs';
import { renderDescription } from '../markup-renderer';

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
});
