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
          // 失敗は throw せず html: null。説明の KFM 表示は付加価値で、
          // 呼び出し側はプレーン表示へ倒せる（エラー表示を増やさない）
          if (!response.ok) return { html: null, source: current.text };

          const data = (await response.json()) as { html?: string | null };
          return { html: data.html ?? null, source: current.text };
        },
        enabled: !!resolved.value,
        // 同じ本文なら結果は変わらない（描画は決定的）。再取得の価値がない
        staleTime: Infinity,
        gcTime: 5 * 60 * 1000,
        retry: false,
      };
    }),
  );
}
