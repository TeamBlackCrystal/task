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
  return (
    (from === 'fixed' && (to === 'verified' || to === 'open')) ||
    (from === 'open' && to === 'rejected') ||
    (from === 'rejected' && to === 'open')
  );
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
export function findingActions(finding: ReviewFinding, viewerId: string): FindingAction[] {
  const labels: Partial<Record<FindingState, string>> = {
    fixed: '修正した',
    verified: '確認した',
    deferred: '繰り延べる',
    rejected: '指摘を取り下げる',
    open: finding.state === 'fixed' ? 'レビューに戻す' : '再オープン',
  };

  return STATES.filter((to) => canTransition(finding.state, to)).map((to) => {
    const selfVerification = to === 'verified' && finding.fixed_by === viewerId;
    return {
      to,
      label: labels[to] ?? STATE_LABELS[to],
      disabledReason: selfVerification ? '自分の修正は自分で確認できません' : null,
    };
  });
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
