import type { components } from '@/generated/api';
import type { TaskGroup } from '@/components/tasks/task-grouped-columns';

type StatusResponse = components['schemas']['ProjectStatusResponse'];
type TaskResponse = components['schemas']['TaskResponse'];

/** 1 ページ分の取得結果。TanStack の `useQueries` の要素から必要な分だけ受ける。 */
export type TaskGroupPage = {
  data?: { tasks: TaskResponse[]; total: number };
  isLoading?: boolean;
  isError?: boolean;
  refetch?: () => unknown;
};

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
  pageSize: number,
): TaskGroup {
  // ページをまたいでタスクが動くと同じ ID が 2 度出ることがあるので落とす
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
  const settled = pages.filter((page) => !!page?.data);
  const lastSettledPage = settled.at(-1)?.data?.tasks;
  const isError = pages.some((page) => !!page?.isError);

  return {
    status,
    tasks,
    total,
    isLoading: pages.some((page) => !!page?.isLoading),
    isError,
    // 最後のページが埋まっていない = 取り切った。total だけで判断すると、件数が
    // 変動したときに減らない「もっと見る」が残る。失敗しているあいだは、穴を
    // 飛ばして先へ進ませないように隠して再試行へ寄せる
    hasMore: !isError && tasks.length < total && lastSettledPage?.length === pageSize,
    retry: () => {
      for (const page of pages) void page?.refetch?.();
    },
  };
}
