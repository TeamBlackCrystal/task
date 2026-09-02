/**
 * POST /internal/render-description — 説明本文の KFM 描画をサーバで行う口。
 *
 * 一覧の分割ビュー（右ペイン）は選択がクライアント操作なので、詳細ページのような
 * server data hook（`@taskId/+data.ts`）が選択中タスクに追従できない。KFM の描画を
 * クライアントへ載せると +417.5 KB になる（kfm-client-registry テスト参照）ため、
 * `/internal/password-strength` と同じ形でサーバへ本文を送り HTML だけを受け取る。
 *
 * 入力は呼び出し側が既に持っている本文なので、この口は権限の境界を動かさない
 * （他人の本文を取得する手段にはならない。描画するのは送られてきた文字列だけ）。
 */
import { Elysia, t } from 'elysia';

import { renderDescription } from '@/lib/markup-renderer';

/**
 * 本文長の上限。`@taskId/+data.ts` の MAX_DESCRIPTION_LENGTH と同じ値・同じ理由で、
 * 超過分は描画せず null を返して呼び出し側のプレーン表示へ倒す。
 * renderDescription の CPU も L1 キャッシュのメモリも本文長に比例するため、
 * 非有界の入力を描画へ入れない。
 */
export const MAX_DESCRIPTION_LENGTH = 65_536;

export const renderDescriptionPlugin = new Elysia().post(
  '/internal/render-description',
  async ({ body }) => {
    const { description, taskId } = body;
    if (!description || description.length > MAX_DESCRIPTION_LENGTH) return { html: null };

    try {
      // scope はタスク UUID で決定的（同一入力 → 同一 HTML）。詳細ページの +data.ts と
      // 同じ組み立てにして、同じタスクなら両経路で脚注 id が一致するようにする。
      // UUID しか受けないので、scope の文字集合制約 [A-Za-z0-9_-]+ は常に満たす。
      return { html: await renderDescription(description, { scope: `task-${taskId}` }) };
    } catch (error) {
      // 描画失敗で 500 を返すと、呼び出し側は説明以外も含めて壊れたように見える。
      // 説明の KFM 表示は付加価値なので null へ倒し、プレーン表示を選ばせる。
      console.error('[render-description] failed', error);
      return { html: null };
    }
  },
  {
    body: t.Object({
      taskId: t.String({ format: 'uuid' }),
      // 上限超過は 422 ではなく html: null で返したいので、ここでは長さを縛らない
      // （リクエスト全体の上限は Elysia / 前段に任せる）。
      description: t.String(),
    }),
  },
);
