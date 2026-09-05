import { useQuery } from '@tanstack/vue-query';
import { computed, toValue, type MaybeRefOrGetter } from 'vue';

/**
 * 説明本文の KFM 描画結果をサーバ（POST /internal/render-description）から取る。
 *
 * 詳細ページは server data hook（`@taskId/+data.ts`）で描画済み HTML を受け取るが、
 * 一覧の分割ビューは選択がクライアント操作なので data hook が追従できない。KFM を
 * クライアントで描画すると +417.5 KB になる（kfm-client-registry テスト参照）ため、
 * 描画はサーバに置いたまま結果だけを取りに行く。
 *
 * 返す `source` は描画に渡した本文そのもの。TaskDetailHub は
 * `descriptionSource === task.description` の厳密一致でしか HTML を v-html へ流さない
 * ので、保存直後や他者更新で古い HTML が出る経路はこの対で塞がる。
 */
export type RenderedDescription = {
  html: string | null;
  source: string;
};

export const RENDER_DESCRIPTION_PATH = '/internal/render-description';

/**
 * その入力では何度試しても成功しない応答。
 *
 * - `411` / `413` — 本文が上限を超えていて、読む前に落とされている
 * - `422` — スキーマ違反（本文長・`taskId` の形式）
 *
 * これらは結果として覚えてよい（プレーン表示のまま再取得しない）。逆に `401` や
 * `5xx` は backend の一時的な不調で出るので、ここに入れない。成功として覚えると
 * `staleTime: Infinity` でプレーン表示に固定され、この PR が直すはずの
 * 「分割ビューが素の Markdown のまま」へ戻ってしまう。
 */
const PERMANENT_FAILURE_STATUSES = new Set([411, 413, 422]);

/** 一時的な失敗を何回まで試すか。諦めたら呼び出し側がプレーン表示へ倒す。 */
const TRANSIENT_FAILURE_RETRIES = 2;

export function renderedDescriptionQueryKey(taskUuid: string, description: string) {
  // 本文そのものをキーに含める。ハッシュにすると衝突時に別タスクの HTML を掴むため、
  // 照合と同じく厳密（衝突なし）にしておく。説明はタスク単位の短文である前提。
  return ['render-description', taskUuid, description] as const;
}

export function useRenderedDescription(
  taskUuid: MaybeRefOrGetter<string | null | undefined>,
  description: MaybeRefOrGetter<string | null | undefined>,
) {
  const resolved = computed(() => {
    const uuid = toValue(taskUuid);
    const text = toValue(description);
    return uuid && text ? { uuid, text } : null;
  });

  return useQuery(
    computed(() => {
      const current = resolved.value;
      return {
        queryKey: renderedDescriptionQueryKey(current?.uuid ?? '', current?.text ?? ''),
        queryFn: async (): Promise<RenderedDescription> => {
          // enabled で絞っているのでここには非 null しか来ないが、型のために畳む
          if (!current) return { html: null, source: '' };

          const response = await fetch(RENDER_DESCRIPTION_PATH, {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({ taskId: current.uuid, description: current.text }),
          });
          if (!response.ok) {
            // 入力由来の拒否は結果として覚える。説明の KFM 表示は付加価値なので、
            // 呼び出し側はプレーン表示へ倒せる（エラー表示を増やさない）
            if (PERMANENT_FAILURE_STATUSES.has(response.status)) {
              return { html: null, source: current.text };
            }
            // 401（backend が不調でセッション確認に失敗した）や 5xx は一時的。
            // ここで html: null を返すと成功として覚えられ、staleTime: Infinity の
            // ぶんプレーン表示に固定される。投げて再試行へ載せる
            throw new Error(`render-description failed with ${response.status}`);
          }

          const data = (await response.json()) as { html?: string | null };
          return { html: data.html ?? null, source: current.text };
        },
        enabled: !!resolved.value,
        // 同じ本文なら結果は変わらない（描画は決定的）。再取得の価値がない。
        // これが効くのは成功した結果だけで、投げた失敗は data を持たないため
        // 次のマウント・フォーカスで取り直される（一時的な不調から戻れる）
        staleTime: Infinity,
        gcTime: 5 * 60 * 1000,
        retry: TRANSIENT_FAILURE_RETRIES,
      };
    }),
  );
}
