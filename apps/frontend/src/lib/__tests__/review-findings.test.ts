import { describe, it, expect } from 'vitest';
import {
  SEVERITIES,
  STATES,
  blocksMerge,
  canDefer,
  canTransition,
  countsAsUnresolved,
  findingActions,
  findingLocation,
  requiresReviewerSide,
  sortFindings,
  summaryRows,
  type FindingSeverity,
  type FindingState,
  type ReviewFinding,
} from '../review-findings';

const VIEWER = '00000000-0000-0000-0000-0000000000aa';
const OTHER = '00000000-0000-0000-0000-0000000000bb';

function finding(overrides: Partial<ReviewFinding> = {}): ReviewFinding {
  return {
    id: 'f-1',
    review_id: 'r-1',
    pr_number: 618,
    round: 1,
    severity: 'high',
    title: '認可漏れ',
    body: '本文',
    file: null,
    line: null,
    state: 'open',
    deferred_task_id: null,
    fixed_by: null,
    created_at: '2026-08-26T00:00:00Z',
    updated_at: '2026-08-26T00:00:00Z',
    transitions: [],
    ...overrides,
  };
}

describe('遷移表', () => {
  // backend の service::reviews::can_transition と同じ表であること。
  // ずれると「押せるのに 409 で失敗する」ボタンができる。
  it('仕様どおりの遷移だけを許す', () => {
    const allowed: [FindingState, FindingState][] = [
      ['open', 'fixed'],
      ['fixed', 'verified'],
      ['fixed', 'open'],
      ['open', 'deferred'],
      ['deferred', 'open'],
      ['open', 'rejected'],
      ['rejected', 'open'],
    ];
    for (const [from, to] of allowed) {
      expect(canTransition(from, to), `${from} -> ${to}`).toBe(true);
    }

    // verified は終端
    for (const to of STATES) {
      expect(canTransition('verified', to), `verified -> ${to}`).toBe(false);
    }
    // 確認を飛ばせない / 繰り延べから直接 fixed にできない / 自分自身へは遷移しない
    expect(canTransition('open', 'verified')).toBe(false);
    expect(canTransition('deferred', 'fixed')).toBe(false);
    for (const state of STATES) {
      expect(canTransition(state, state), `${state} -> ${state}`).toBe(false);
    }
  });

  it('レビュー側限定の遷移は確認・差し戻し・棄却', () => {
    expect(requiresReviewerSide('fixed', 'verified')).toBe(true);
    expect(requiresReviewerSide('fixed', 'open')).toBe(true);
    expect(requiresReviewerSide('open', 'rejected')).toBe(true);
    expect(requiresReviewerSide('rejected', 'open')).toBe(true);
    // 修正の宣言と繰り延べの出入りは修正側も行える
    expect(requiresReviewerSide('open', 'fixed')).toBe(false);
    expect(requiresReviewerSide('open', 'deferred')).toBe(false);
    expect(requiresReviewerSide('deferred', 'open')).toBe(false);
  });
});

describe('canDefer', () => {
  it('繰り延べられるのはマージを塞がない重大度だけ', () => {
    expect(canDefer('low')).toBe(true);
    expect(canDefer('nit')).toBe(true);
    expect(canDefer('high')).toBe(false);
    expect(canDefer('medium')).toBe(false);
    // backend の can_defer と同じく blocks_merge の裏返し
    for (const severity of SEVERITIES) {
      expect(canDefer(severity)).toBe(!blocksMerge(severity));
    }
  });
});

describe('findingActions', () => {
  it('open の Low では修正・繰り延べ・取り下げを出す', () => {
    const actions = findingActions(finding({ severity: 'low' }), VIEWER);
    expect(actions.map((a) => a.to)).toEqual(['fixed', 'deferred', 'rejected']);
    expect(actions.every((a) => a.disabledReason === null)).toBe(true);
  });

  it('High / Medium には繰り延べを出さない（サーバーも 409 で拒否する）', () => {
    for (const severity of ['high', 'medium'] as const) {
      const actions = findingActions(finding({ severity }), VIEWER);
      expect(actions.map((a) => a.to)).toEqual(['fixed', 'rejected']);
    }
  });

  it('fixed を宣言した本人には verified を押させない', () => {
    const actions = findingActions(finding({ state: 'fixed', fixed_by: VIEWER }), VIEWER);
    const verify = actions.find((a) => a.to === 'verified');
    expect(verify?.disabledReason).toBe('自分の修正は自分で確認できません');
    // 差し戻しは押せる
    expect(actions.find((a) => a.to === 'open')?.disabledReason).toBeNull();
  });

  it('別の人が直した指摘は確認できる', () => {
    const actions = findingActions(finding({ state: 'fixed', fixed_by: OTHER }), VIEWER);
    expect(actions.find((a) => a.to === 'verified')?.disabledReason).toBeNull();
  });

  it('verified には操作が無い（終端）', () => {
    expect(findingActions(finding({ state: 'verified' }), VIEWER)).toEqual([]);
  });
});

describe('マージ判定の材料', () => {
  it('High / Medium だけがマージを塞ぐ', () => {
    expect(SEVERITIES.filter(blocksMerge)).toEqual(['high', 'medium']);
  });

  it('open と fixed を未解決に数える（fixed は確認が済んでいない）', () => {
    expect(STATES.filter(countsAsUnresolved)).toEqual(['open', 'fixed']);
  });
});

describe('summaryRows', () => {
  it('件数 0 の組み合わせは出さず、重大度 → 状態の順に並べる', () => {
    const rows = summaryRows({
      pr_number: 618,
      rounds: 2,
      blocking: 2,
      mergeable: false,
      counts: [
        { severity: 'low', state: 'deferred', count: 3 },
        { severity: 'high', state: 'open', count: 2 },
        { severity: 'medium', state: 'verified', count: 0 },
      ],
    });
    expect(rows).toEqual([
      { severity: 'high', state: 'open', count: 2 },
      { severity: 'low', state: 'deferred', count: 3 },
    ]);
  });
});

describe('表示ヘルパー', () => {
  it('位置は file:line、行が無ければ file だけ、file が無ければ null', () => {
    expect(findingLocation(finding({ file: 'src/App.vue', line: 42 }))).toBe('src/App.vue:42');
    expect(findingLocation(finding({ file: 'src/App.vue' }))).toBe('src/App.vue');
    expect(findingLocation(finding())).toBeNull();
  });

  it('重大度が高い順 → 未解決を先に → 新しいラウンドを先に並べる', () => {
    const sorted = sortFindings([
      finding({ id: 'nit', severity: 'nit' as FindingSeverity }),
      finding({ id: 'high-verified', severity: 'high', state: 'verified' }),
      finding({ id: 'high-open-r1', severity: 'high', state: 'open', round: 1 }),
      finding({ id: 'high-open-r2', severity: 'high', state: 'open', round: 2 }),
      finding({ id: 'medium', severity: 'medium' }),
    ]);
    expect(sorted.map((f) => f.id)).toEqual([
      'high-open-r2',
      'high-open-r1',
      'high-verified',
      'medium',
      'nit',
    ]);
  });
});
