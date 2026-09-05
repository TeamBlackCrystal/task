import type { components } from '@/generated/api';
import type { TaskGroup } from '@/components/tasks/task-grouped-columns';

type StatusResponse = components['schemas']['ProjectStatusResponse'];
type TaskResponse = components['schemas']['TaskResponse'];

/** 1 ページ分のレスポンス。 */
export type TaskGroupPage = {
  tasks: TaskResponse[];
  total: number;
  next_cursor: string | null;
};

/**
 * ステータス 1 つ分の infinite query から、一覧が使う分だけ受ける形。
 *
 * ページを別々のクエリにすると、後続ページのキーに取得時点のカーソルが焼き付く。
 * 先頭ページが取り直されて中身が変わっても後続は古い鍵のまま引くので、
 * 境界のタスクがどのページにも出なくなる。infinite query はページを順に引き直し、
 * 鍵をそのつど前のページから採り直すので、取り直しでも並びが繋がったままになる。
 */
export type TaskGroupQueryState = {
  data?: { pages: TaskGroupPage[] };
  isLoading?: boolean;
  isFetchingNextPage?: boolean;
  isError?: boolean;
  hasNextPage?: boolean;
  refetch?: () => unknown;
  fetchNextPage?: () => unknown;
};

/**
 * ステータス 1 つ分の取得結果を、一覧が使う塊にまとめる。
 *
 * 「もっと見る」の出し入れは間違えやすいので、コンポーネントから切り出してテストする。
 */
export function toTaskGroup(status: StatusResponse, query: TaskGroupQueryState): TaskGroup {
  const pages = query.data?.pages ?? [];

  // カーソルは created_at / id で継ぐので、並びの中でタスクが動くことは無い。
  // ただし優先度・期限で並べ替えるとキー自体が動くので、保険として ID の重複は落とす
  const seen = new Set<string>();
  const tasks = pages
    .flatMap((page) => page.tasks)
    .filter((task) => {
      if (seen.has(task.id)) return false;
      seen.add(task.id);
      return true;
    });

  const isError = !!query.isError;

  return {
    status,
    tasks,
    // total は後のページほど新しいので、返ってきた最後の値を採る
    total: pages.at(-1)?.total ?? 0,
    // 次ページの取得中も「読み込み中」に含める。押した直後に何も起きていないように
    // 見えると、続けて押されて同じページが二重に積まれる
    isLoading: !!query.isLoading || !!query.isFetchingNextPage,
    isError,
    // 続きの有無はサーバの next_cursor だけで決める（infinite query が
    // getNextPageParam で畳んだ結果を見る）。取得済み件数と total の比較でやると、
    // 読んでいるあいだに件数が動くだけで判定が狂う。失敗しているあいだは、穴を
    // 飛ばして先へ進ませないように隠して再試行へ寄せる
    hasMore: !isError && !!query.hasNextPage,
    retry: () => void query.refetch?.(),
    loadMore: () => void query.fetchNextPage?.(),
  };
}
