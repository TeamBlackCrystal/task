import type { components } from '@/generated/api';

export type FindingSeverity = components['schemas']['FindingSeverity'];
export type FindingState = components['schemas']['FindingState'];
export type ReviewFinding = components['schemas']['FindingResponse'];
export type Review = components['schemas']['ReviewResponse'];
export type ReviewedPullRequest = components['schemas']['ReviewedPullRequest'];
export type ReviewSummary = components['schemas']['ReviewSummaryResponse'];

export const SEVERITIES: FindingSeverity[] = ['high', 'medium', 'low', 'nit'];
export const STATES: FindingState[] = ['open', 'fixed', 'verified', 'deferred', 'rejected'];

export const SEVERITY_LABELS: Record<FindingSeverity, string> = {
  high: 'High',
  medium: 'Medium',
  low: 'Low',
  nit: 'Nit',
};

export const STATE_LABELS: Record<FindingState, string> = {
  open: 'Open',
  fixed: 'Fixed',
  verified: 'Verified',
  deferred: 'Deferred',
  rejected: 'Rejected',
};

/** マージ前に解消が必要な重大度（backend の `FindingSeverity::blocks_merge` と対）。 */
export function blocksMerge(severity: FindingSeverity): boolean {
  return severity === 'high' || severity === 'medium';
}

/**
 * 繰り延べ（deferred）を許す重大度（backend の `FindingSeverity::can_defer` と対）。
 *
 * 繰り延べはマージ可否の集計から外れるので、マージ前必須の重大度には許さない。
 * サーバーも 409 で拒否する。
 */
export function canDefer(severity: FindingSeverity): boolean {
  return !blocksMerge(severity);
}

/** マージ判定で「未解決」と数える状態（backend の `counts_as_unresolved` と対）。 */
export function countsAsUnresolved(state: FindingState): boolean {
  return state === 'open' || state === 'fixed';
}

/**
 * 遷移そのものが許されるか。backend の `service::reviews::can_transition` と同じ表。
 *
 * 画面側で先に弾くのは、押しても 409 になるだけのボタンを出さないため。
 * ずれると「押せるのに失敗する」ボタンができるので、変えるときは両方直す。
 */
export function canTransition(from: FindingState, to: FindingState): boolean {
  const table: Record<FindingState, FindingState[]> = {
    open: ['fixed', 'deferred', 'rejected'],
    fixed: ['verified', 'open'],
    // verified は終端。誤りだったと分かったら新しいラウンドで出し直す
    verified: [],
    deferred: ['open'],
    rejected: ['open'],
  };
  return table[from].includes(to);
}

/** レビュー側だけが行える遷移か（backend の `requires_reviewer_side` と対）。 */
export function requiresReviewerSide(from: FindingState, to: FindingState): boolean {
  return from === 'fixed' && (to === 'verified' || to === 'open');
}

/**
 * 指摘を出したラウンドの作成者だけが行える遷移か
 * （backend の `requires_finding_author` と対）。
 *
 * 取り下げだけレビュー側より狭い。ラウンドは指摘ゼロでも作れるので、後から出した
 * ラウンドの作成者にまで認めると、空のラウンドを 1 本作るだけで他人の High を
 * 棄却でき、マージ基準を 1 人で迂回できてしまう。
 */
export function requiresFindingAuthor(from: FindingState, to: FindingState): boolean {
  return (from === 'open' && to === 'rejected') || (from === 'rejected' && to === 'open');
}

export type FindingAction = {
  to: FindingState;
  label: string;
  /** 押せない理由。null なら押せる */
  disabledReason: string | null;
};

/**
 * 指摘の現在の状態から、画面に出す操作を導く。
 *
 * `viewerId` が `fixed` を宣言した本人なら verified は押せない
 * （自分の修正を自分で確認済みにはできない）。
 */
export function findingActions(
  finding: ReviewFinding,
  viewerId: string,
  /** その指摘を出したラウンドの作成者。分からなければ取り下げを出さない */
  findingAuthorId?: string | null,
  /**
   * 閲覧者がこの指摘のレビュー側か（その指摘のラウンド以降のラウンドを出しているか）。
   * ラウンド一覧が引けないうちは false 側に倒し、レビュー側限定の操作を出さない
   */
  isReviewerSide = false,
): FindingAction[] {
  const labels: Partial<Record<FindingState, string>> = {
    fixed: '修正した',
    verified: '確認した',
    deferred: '繰り延べる',
    rejected: '指摘を取り下げる',
    open: finding.state === 'fixed' ? 'レビューに戻す' : '再オープン',
  };

  return STATES.filter(
    (to) =>
      canTransition(finding.state, to) &&
      // High / Medium は繰り延べられない（押しても 409 になるボタンを出さない）
      (to !== 'deferred' || canDefer(finding.severity)) &&
      // 取り下げは指摘を出した本人だけ（押しても 403 になるボタンを出さない）
      (!requiresFindingAuthor(finding.state, to) || viewerId === findingAuthorId) &&
      // 確認と差し戻しはレビュー側だけ（同上。修正だけを行う人には出さない）
      (!requiresReviewerSide(finding.state, to) || isReviewerSide),
  ).map((to) => {
    const selfVerification = to === 'verified' && finding.fixed_by === viewerId;
    return {
      to,
      label: labels[to] ?? STATE_LABELS[to],
      disabledReason: selfVerification ? '自分の修正は自分で確認できません' : null,
    };
  });
}

/**
 * マージ可否の見出しと説明。
 *
 * 「レビューが 1 件も無い」と「レビュー済みで指摘なし」は別物なので出し分ける
 * （backend も未レビューの PR を mergeable にしない）。レビューした commit は
 * 出しておく——手元の HEAD と見比べれば、レビュー後に積まれたコミットに気づける。
 */
export type MergeVerdictKind =
  | 'unlinked'
  | 'unreviewed'
  | 'blocked'
  | 'stale'
  | 'unknown-freshness'
  | 'mergeable';

/** 「可」を出してよいのは `mergeable` だけ。ほかはすべてゲートとして通さない。 */
export function mergeVerdict(summary: ReviewSummary): {
  kind: MergeVerdictKind;
  title: string;
  detail: string;
} {
  // 連携が無いと集計の視界が空になり、空のラウンド 1 本で「可」を作れてしまう。
  // CLI だけで塞いでも、人間の主経路である画面が素通しなら意味がない（仕様 §8）
  if (!summary.repository) {
    return {
      kind: 'unlinked',
      title: 'リポジトリ未確定',
      detail:
        'GitHub 連携が無いため、どのリポジトリの PR を見た集計か決まりません。マージの判断には使えません',
    };
  }
  if (summary.rounds === 0) {
    return {
      kind: 'unreviewed',
      title: '未レビュー',
      detail: 'まだレビューされていません。レビューを 1 ラウンド出してください',
    };
  }
  const reviewed = summary.latest_head_sha
    ? `最新ラウンドは ${summary.latest_head_sha.slice(0, 7)} を見ています`
    : '';
  if (!summary.mergeable) {
    return {
      kind: 'blocked',
      title: `マージ不可（${summary.blocking} 件）`,
      detail: ['High / Medium が未解決です。Low / Nit は繰り延べできます', reviewed]
        .filter(Boolean)
        .join(' · '),
    };
  }
  // ここから先は未解決ゼロ。レビュー後にコミットが積まれていないかを見る。
  // 判定は片道降格——「可」以外へ落とすのにだけ使い、「可」の保証には使わない
  if (!summary.cached_pr_head_sha) {
    return {
      kind: 'unknown-freshness',
      title: '鮮度不明',
      detail: ['High / Medium の未解決はありませんが、現在の HEAD を確認できていません', reviewed]
        .filter(Boolean)
        .join(' · '),
    };
  }
  if (summary.cached_pr_head_sha !== summary.latest_head_sha) {
    return {
      kind: 'stale',
      title: 'レビューが古い',
      detail: [
        `レビュー後にコミットが積まれています（現在 ${summary.cached_pr_head_sha.slice(0, 7)}）`,
        reviewed,
      ]
        .filter(Boolean)
        .join(' · '),
    };
  }
  // キャッシュは push では更新されないので、いつ時点の確認かを必ず添える
  const checkedAt = summary.pr_head_checked_at
    ? `GitHub 確認: ${new Date(summary.pr_head_checked_at).toLocaleString('ja-JP')} 時点`
    : '';
  return {
    kind: 'mergeable',
    title: 'マージ可',
    detail: ['High / Medium の未解決はありません', reviewed, checkedAt].filter(Boolean).join(' · '),
  };
}

/** 指摘の位置（`file:line`）。位置情報が無ければ null。 */
export function findingLocation(finding: ReviewFinding): string | null {
  if (!finding.file) return null;
  return finding.line ? `${finding.file}:${finding.line}` : finding.file;
}

/** 一覧の並び: 重大度が高い順 → 未解決を先に → 新しいラウンドを先に。 */
export function sortFindings(findings: ReviewFinding[]): ReviewFinding[] {
  const severityRank = (severity: FindingSeverity) => SEVERITIES.indexOf(severity);
  return [...findings].sort(
    (a, b) =>
      severityRank(a.severity) - severityRank(b.severity) ||
      Number(countsAsUnresolved(b.state)) - Number(countsAsUnresolved(a.state)) ||
      b.round - a.round,
  );
}

/** 集計から「重大度 → 状態 → 件数」の表示用の行を作る（件数 0 は出さない）。 */
export function summaryRows(
  summary: ReviewSummary,
): { severity: FindingSeverity; state: FindingState; count: number }[] {
  const rows: { severity: FindingSeverity; state: FindingState; count: number }[] = [];
  for (const severity of SEVERITIES) {
    for (const state of STATES) {
      const count = summary.counts
        .filter((entry) => entry.severity === severity && entry.state === state)
        .reduce((total, entry) => total + entry.count, 0);
      if (count > 0) rows.push({ severity, state, count });
    }
  }
  return rows;
}
