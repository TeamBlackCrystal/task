import type { components } from '@/generated/api';
import type { TaskGroup } from '@/components/tasks/task-grouped-columns';

type StatusResponse = components['schemas']['ProjectStatusResponse'];
type TaskResponse = components['schemas']['TaskResponse'];

/** 1 ページ分の取得結果。TanStack の `useQueries` の要素から必要な分だけ受ける。 */
export type TaskGroupPage = {
  data?: { tasks: TaskResponse[]; total: number; next_cursor?: string | null };
  isLoading?: boolean;
  isError?: boolean;
  refetch?: () => unknown;
};

/**
 * そのステータスで次に読むページの鍵。取り切っていれば `null`。
 *
 * 「もっと見る」はこれを積んでページを増やす。返ってきた最後のページから採るので、
 * 取得中・失敗中のページは起点にしない。
 */
export function nextGroupCursor(pages: (TaskGroupPage | null | undefined)[]): string | null {
  const settled = pages.filter((page) => !!page?.data);
  return settled.at(-1)?.data?.next_cursor ?? null;
}

/**
 * ステータス 1 つ分のページ群を、一覧が使う塊にまとめる。
 *
 * 「もっと見る」の出し入れは間違えやすいので、+Page.vue から切り出してテストする。
 * 判定は **返ってきたページだけ** で行う。取得中のページを混ぜると、押した直後に
 * ボタンが消え、失敗したときは戻ってこない。
 */
export function toTaskGroup(
  status: StatusResponse,
  pages: (TaskGroupPage | null | undefined)[],
): TaskGroup {
  // カーソルは created_at / id で継ぐので、並びの中でタスクが動くことは無い。
  // ただし優先度・期限で並べ替えるとキー自体が動くので、保険として ID の重複は落とす
  const seen = new Set<string>();
  const tasks = pages
    .flatMap((page) => page?.data?.tasks ?? [])
    .filter((task) => {
      if (seen.has(task.id)) return false;
      seen.add(task.id);
      return true;
    });

  // total は後のページほど新しいので、返ってきた最後の値を採る
  const total = pages.reduce((acc, page) => page?.data?.total ?? acc, 0);
  const isError = pages.some((page) => !!page?.isError);

  return {
    status,
    tasks,
    total,
    isLoading: pages.some((page) => !!page?.isLoading),
    isError,
    // 続きの有無はサーバの next_cursor だけで決める。取得済み件数と total の比較で
    // やると、読んでいるあいだに件数が動くだけで判定が狂う。失敗しているあいだは、
    // 穴を飛ばして先へ進ませないように隠して再試行へ寄せる
    hasMore: !isError && nextGroupCursor(pages) !== null,
    retry: () => {
      for (const page of pages) void page?.refetch?.();
    },
  };
}
