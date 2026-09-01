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
  requiresFindingAuthor,
  requiresReviewerSide,
  sortFindings,
  mergeVerdict,
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

  it('レビュー側限定の遷移は確認と差し戻し', () => {
    expect(requiresReviewerSide('fixed', 'verified')).toBe(true);
    expect(requiresReviewerSide('fixed', 'open')).toBe(true);
    // 修正の宣言と繰り延べの出入りは修正側も行える
    expect(requiresReviewerSide('open', 'fixed')).toBe(false);
    expect(requiresReviewerSide('open', 'deferred')).toBe(false);
    expect(requiresReviewerSide('deferred', 'open')).toBe(false);
  });

  it('取り下げはレビュー側より狭く、指摘を出した本人に限る', () => {
    expect(requiresFindingAuthor('open', 'rejected')).toBe(true);
    expect(requiresFindingAuthor('rejected', 'open')).toBe(true);
    // 緩い方には載せない（二重判定にしない）
    expect(requiresReviewerSide('open', 'rejected')).toBe(false);
    expect(requiresReviewerSide('rejected', 'open')).toBe(false);
    // 確認と差し戻しは後続ラウンドの作成者にも許す
    expect(requiresFindingAuthor('fixed', 'verified')).toBe(false);
    expect(requiresFindingAuthor('fixed', 'open')).toBe(false);
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
    const actions = findingActions(finding({ severity: 'low' }), VIEWER, VIEWER);
    expect(actions.map((a) => a.to)).toEqual(['fixed', 'deferred', 'rejected']);
    expect(actions.every((a) => a.disabledReason === null)).toBe(true);
  });

  it('High / Medium には繰り延べを出さない（サーバーも 409 で拒否する）', () => {
    for (const severity of ['high', 'medium'] as const) {
      const actions = findingActions(finding({ severity }), VIEWER, VIEWER);
      expect(actions.map((a) => a.to)).toEqual(['fixed', 'rejected']);
    }
  });

  it('取り下げは指摘を出した本人にだけ出す（サーバーも 403 で拒否する）', () => {
    // 他人が出した指摘。空のラウンドを作って「レビュー側」を名乗っても取り下げられない
    const others = findingActions(finding({ severity: 'low' }), VIEWER, OTHER);
    expect(others.map((a) => a.to)).toEqual(['fixed', 'deferred']);

    // 出した本人には出す
    const own = findingActions(finding({ severity: 'low' }), VIEWER, VIEWER);
    expect(own.map((a) => a.to)).toContain('rejected');

    // ラウンドが引けないうちは出さない（押せるのに 403 になるボタンを作らない）
    const unknown = findingActions(finding({ severity: 'low' }), VIEWER, null);
    expect(unknown.map((a) => a.to)).not.toContain('rejected');

    // rejected からの再オープンも同じ主体に限る
    expect(findingActions(finding({ state: 'rejected' }), VIEWER, OTHER).map((a) => a.to)).toEqual(
      [],
    );
    expect(findingActions(finding({ state: 'rejected' }), VIEWER, VIEWER).map((a) => a.to)).toEqual(
      ['open'],
    );
  });

  it('作成者が不在ならオーナーの代行で取り下げを出す（サーバーも同じ例外を持つ）', () => {
    const others = finding({ severity: 'low' });

    // 他人が出した指摘。代行が立たないうちは出さない
    expect(findingActions(others, VIEWER, OTHER, false, false).map((a) => a.to)).toEqual([
      'fixed',
      'deferred',
    ]);

    // 代行が立てば取り下げを出す
    expect(findingActions(others, VIEWER, OTHER, false, true).map((a) => a.to)).toContain(
      'rejected',
    );

    // rejected からの再オープンも同じ例外に乗る（往復できないと戻せなくなる）
    expect(
      findingActions(finding({ state: 'rejected' }), VIEWER, OTHER, false, true).map((a) => a.to),
    ).toEqual(['open']);
  });

  it('fixed を宣言した本人には verified を押させない', () => {
    const actions = findingActions(
      finding({ state: 'fixed', fixed_by: VIEWER }),
      VIEWER,
      null,
      true,
    );
    const verify = actions.find((a) => a.to === 'verified');
    expect(verify?.disabledReason).toBe('自分の修正は自分で確認できません');
    // 差し戻しは押せる
    expect(actions.find((a) => a.to === 'open')?.disabledReason).toBeNull();
  });

  it('別の人が直した指摘はレビュー側なら確認できる', () => {
    const actions = findingActions(
      finding({ state: 'fixed', fixed_by: OTHER }),
      VIEWER,
      null,
      true,
    );
    expect(actions.find((a) => a.to === 'verified')?.disabledReason).toBeNull();
  });

  it('確認と差し戻しはレビュー側にだけ出す（サーバーも 403 で拒否する）', () => {
    const fixed = finding({ state: 'fixed', fixed_by: OTHER });

    // 修正だけを行う人。同僚が宣言した fixed でも、押せば 403 になるので出さない
    expect(findingActions(fixed, VIEWER, null, false).map((a) => a.to)).toEqual([]);

    // レビュー側には確認と差し戻しの両方を出す
    expect(findingActions(fixed, VIEWER, null, true).map((a) => a.to)).toEqual([
      'open',
      'verified',
    ]);

    // 引数を省いたときも出さない側に倒す（ラウンド一覧が引けていない状態）
    expect(findingActions(fixed, VIEWER, null).map((a) => a.to)).toEqual([]);
  });

  it('レビュー側でなくても修正の宣言と繰り延べは出す', () => {
    // requires_reviewer_side は fixed からの 2 遷移だけ。open からの操作は狭めない
    const actions = findingActions(finding({ severity: 'low' }), VIEWER, VIEWER, false);
    expect(actions.map((a) => a.to)).toEqual(['fixed', 'deferred', 'rejected']);
  });

  it('verified には操作が無い（終端）', () => {
    expect(findingActions(finding({ state: 'verified' }), VIEWER, VIEWER)).toEqual([]);
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

describe('mergeVerdict', () => {
  const REVIEWED = '60cdd7795f94fa4e4148ce996c2efb4c363e3f5e';

  const summary = (over: Partial<Parameters<typeof mergeVerdict>[0]>) =>
    mergeVerdict({
      pr_number: 618,
      rounds: 1,
      counts: [],
      blocking: 0,
      latest_head_sha: REVIEWED,
      // 既定は「連携あり・レビューした commit が現在の head」＝可を出してよい状態
      repository: 'acme/app',
      cached_pr_head_sha: REVIEWED,
      pr_head_checked_at: '2026-08-28T10:00:00Z',
      owner_override_rejections: 0,
      mergeable: true,
      ...over,
    });

  it('連携が無ければ「可」と言わない（集計の視界が空になるため）', () => {
    const verdict = summary({ repository: null });
    expect(verdict.kind).toBe('unlinked');
    expect(verdict.title).toBe('リポジトリ未確定');
  });

  it('レビューが 1 件も無い PR は「可」と言わない', () => {
    const verdict = summary({ rounds: 0, latest_head_sha: null, mergeable: false });
    expect(verdict.kind).toBe('unreviewed');
    expect(verdict.title).toBe('未レビュー');
    expect(verdict.detail).toContain('まだレビューされていません');
  });

  it('レビュー後にコミットが積まれていれば「可」と言わない', () => {
    const verdict = summary({ cached_pr_head_sha: 'ffffffffffffffffffffffffffffffffffffffff' });
    expect(verdict.kind).toBe('stale');
    expect(verdict.title).toBe('レビューが古い');
    expect(verdict.detail).toContain('fffffff');
  });

  it('現在の HEAD を確かめられていなければ「可」と言わない', () => {
    const verdict = summary({ cached_pr_head_sha: null });
    expect(verdict.kind).toBe('unknown-freshness');
    expect(verdict.title).toBe('鮮度不明');
  });

  it('可のときはレビューした commit と確認時刻を添える', () => {
    const verdict = summary({});
    expect(verdict.kind).toBe('mergeable');
    expect(verdict.title).toBe('マージ可');
    expect(verdict.detail).toContain('60cdd77');
    // キャッシュは push で更新されないので、いつ時点の確認かを必ず出す
    expect(verdict.detail).toContain('GitHub 確認');
  });

  it('未解決が残っていれば件数つきで不可', () => {
    const verdict = summary({ blocking: 2, mergeable: false });
    expect(verdict.kind).toBe('blocked');
    expect(verdict.title).toBe('マージ不可（2 件）');
    expect(verdict.detail).toContain('60cdd77');
  });
});

describe('summaryRows', () => {
  it('件数 0 の組み合わせは出さず、重大度 → 状態の順に並べる', () => {
    const rows = summaryRows({
      pr_number: 618,
      rounds: 2,
      blocking: 2,
      latest_head_sha: null,
      repository: 'acme/app',
      cached_pr_head_sha: null,
      pr_head_checked_at: null,
      owner_override_rejections: 0,
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
