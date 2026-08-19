import { describe, expect, it } from 'vitest';
import { KFM_MERMAID_STORY_INPUTS } from '../kfm-story-fixtures/inputs-mermaid';
import { renderDescription } from '../markup-renderer';

/**
 * KFM mermaid story fixture の drift 検査 (cmd_670 方式と同型)。
 * stories/kfm/KfmMermaid.stories.ts が v-html する HTML fixture
 * (kfm-story-fixtures/rendered/mermaid-*.html) は renderDescription の事前生成物
 * (= SSR が出す不活性 <kfm-mermaid>。SVG は含まない)。レンダラ出力が変わったのに
 * fixture が古いままだと VRT baseline が「本番と違う姿」を守り続けるため、
 * 現在出力との一致を CI で強制する。再生成: pnpm test:unit --update
 */
describe('KFM mermaid story fixtures (drift 検査)', () => {
  it.each(Object.entries(KFM_MERMAID_STORY_INPUTS))(
    'fixture %s は renderDescription の現在出力と一致する',
    async (name, input) => {
      await expect(await renderDescription(input)).toMatchFileSnapshot(
        `../kfm-story-fixtures/rendered/${name}.html`,
      );
    },
  );
});
