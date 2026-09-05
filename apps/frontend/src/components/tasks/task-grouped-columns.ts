import type { components } from '@/generated/api';

type StatusResponse = components['schemas']['ProjectStatusResponse'];
type TaskResponse = components['schemas']['TaskResponse'];

/** ステータス 1 つ分の塊。件数はサーバの total（取得済み件数ではない）。 */
export type TaskGroup = {
  status: StatusResponse;
  tasks: TaskResponse[];
  total: number;
  isLoading: boolean;
  isError: boolean;
  /** まだ取れていない件があるか */
  hasMore: boolean;
  /** 取得に失敗したページを取り直す。失敗したままだと先へ進めないので導線を出す */
  retry: () => void;
  /** 次のページを足す。カーソルはグループ自身が持つので、呼ぶ側は鍵を知らなくてよい */
  loadMore: () => void;
};

/**
 * List 表示の列定義。
 *
 * ヘッダー（TaskGroupedList）と行（TaskGroupedRow）は別コンポーネントだが、
 * 同じグリッドに載っていないと列がずれる。片方だけ直す事故を防ぐためここに置く。
 *
 * ステータス列は持たない。グループがステータスそのものなので列にすると重複し、
 * 変更はタスク名の左の丸から行う（参照デザイン）。
 * 担当者は複数付くので、アバター 3 つ＋「+N」が収まる幅を取る。
 */
export const TASK_ROW_GRID =
  'grid min-w-[42rem] grid-cols-[minmax(0,1fr)_7rem_7rem_6rem_4rem] items-center';
