// @vitest-environment node
import { Elysia } from 'elysia';
import { describe, expect, it } from 'vitest';

import { MAX_DESCRIPTION_LENGTH, renderDescriptionPlugin } from '../render-description';

const app = new Elysia().use(renderDescriptionPlugin);

const TASK_UUID = '00000000-0000-0000-0000-000000000042';

async function post(body: unknown): Promise<Response> {
  return app.handle(
    new Request('http://localhost/internal/render-description', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(body),
    }),
  );
}

describe('POST /internal/render-description', () => {
  it('markdown を KFM の HTML にして返す', async () => {
    const response = await post({ taskId: TASK_UUID, description: '# 見出し\n\n- 箇条書き' });
    expect(response.status).toBe(200);

    const { html } = (await response.json()) as { html: string | null };
    // 素の markdown がそのまま出ていたのが直したい症状なので、記法が要素になっていることを見る
    expect(html).toContain('<h1');
    expect(html).toContain('<li>');
    expect(html).not.toContain('# 見出し');
  });

  it('脚注の id はタスク UUID で scope 化する（詳細ページと同じ組み立て）', async () => {
    const response = await post({
      taskId: TASK_UUID,
      description: '本文[^1]\n\n[^1]: 注記',
    });

    const { html } = (await response.json()) as { html: string | null };
    // 同一ページに複数の KFM 断片が並んだときに id が衝突しないための scope
    expect(html).toContain(`user-content-task-${TASK_UUID}-`);
  });

  it('空の本文は描画せず null を返す', async () => {
    const response = await post({ taskId: TASK_UUID, description: '' });

    expect(response.status).toBe(200);
    expect(await response.json()).toEqual({ html: null });
  });

  it('上限を超える本文は描画せず null を返す（SSR に非有界の入力を入れない）', async () => {
    const response = await post({
      taskId: TASK_UUID,
      description: 'a'.repeat(MAX_DESCRIPTION_LENGTH + 1),
    });

    expect(response.status).toBe(200);
    expect(await response.json()).toEqual({ html: null });
  });

  it('上限ちょうどの本文は描画する', async () => {
    const response = await post({
      taskId: TASK_UUID,
      description: 'a'.repeat(MAX_DESCRIPTION_LENGTH),
    });

    const { html } = (await response.json()) as { html: string | null };
    expect(html).toContain('<p>');
  });

  it('UUID でない taskId は受け付けない（scope の文字集合を壊さない）', async () => {
    const response = await post({ taskId: 'task-1 onerror=x', description: '本文' });

    expect(response.status).toBeGreaterThanOrEqual(400);
  });

  it('script は sanitize で落とす', async () => {
    const response = await post({
      taskId: TASK_UUID,
      description: '<script>alert(1)</script>\n\n本文',
    });

    const { html } = (await response.json()) as { html: string | null };
    expect(html).not.toContain('<script');
  });
});
