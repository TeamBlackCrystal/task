import { describe, expect, it, vi } from 'vitest';
import { mount } from '@vue/test-utils';
import { defineComponent, nextTick, ref } from 'vue';
import type { SortingState } from '@tanstack/vue-table';

import {
  applyTaskListUrlState,
  clampPage,
  DEFAULT_TASK_LIST_URL_STATE,
  parseTaskListUrlState,
  useTaskListUrlSync,
} from '../task-list-url-state';

const BASE = 'https://app.example.com/acme/projects/ENG/tasks';

describe('parseTaskListUrlState', () => {
  it('クエリが無ければ既定値', () => {
    expect(parseTaskListUrlState(undefined)).toEqual(DEFAULT_TASK_LIST_URL_STATE);
    expect(parseTaskListUrlState({})).toEqual(DEFAULT_TASK_LIST_URL_STATE);
  });

  it('ページ・検索語・ラベル・並び替えを読む', () => {
    expect(
      parseTaskListUrlState({
        page: '3',
        q: 'oauth',
        label: 'label-1',
        sort: 'title:asc,priority:desc',
      }),
    ).toEqual({
      page: 3,
      q: 'oauth',
      labelId: 'label-1',
      sorting: [
        { id: 'title', desc: false },
        { id: 'priority', desc: true },
      ],
    });
  });

  it('検索語の前後の空白は落とす（空白だけなら検索していない扱い）', () => {
    expect(parseTaskListUrlState({ q: '  oauth  ' }).q).toBe('oauth');
    expect(parseTaskListUrlState({ q: '   ' }).q).toBe('');
  });

  // 細工されたクエリで負の offset や不正な sorting を作らせない
  it.each([
    ['0', 1],
    ['-2', 1],
    ['1.5', 1],
    ['abc', 1],
    ['', 1],
    ['2', 2],
    // 境界のすぐ外側も見る（1 ページ目だけを固定値にしない）
    ['21', 21],
  ])('page=%s は %i になる', (raw, expected) => {
    expect(parseTaskListUrlState({ page: raw }).page).toBe(expected);
  });

  it('向きが asc/desc でない並び替えは捨てる', () => {
    expect(parseTaskListUrlState({ sort: 'title:sideways' }).sorting).toEqual([]);
    expect(parseTaskListUrlState({ sort: ':asc' }).sorting).toEqual([]);
    expect(parseTaskListUrlState({ sort: 'title' }).sorting).toEqual([]);
    // 壊れた項目だけ落として残りは活かす
    expect(parseTaskListUrlState({ sort: 'title:asc,broken' }).sorting).toEqual([
      { id: 'title', desc: false },
    ]);
  });

  it('空のラベルは「すべて」として扱う', () => {
    expect(parseTaskListUrlState({ label: '' }).labelId).toBeNull();
  });
});

describe('applyTaskListUrlState', () => {
  it('既定値のキーは URL に出さない', () => {
    const url = applyTaskListUrlState(new URL(BASE), DEFAULT_TASK_LIST_URL_STATE);
    expect(url.search).toBe('');
  });

  it('既定でない値だけを載せる', () => {
    const url = applyTaskListUrlState(new URL(BASE), {
      page: 3,
      q: 'oauth',
      labelId: 'label-1',
      sorting: [{ id: 'title', desc: true }],
    });
    expect(url.searchParams.get('page')).toBe('3');
    expect(url.searchParams.get('q')).toBe('oauth');
    expect(url.searchParams.get('label')).toBe('label-1');
    expect(url.searchParams.get('sort')).toBe('title:desc');
  });

  it('既定へ戻した項目は URL から消える', () => {
    const dirty = new URL(`${BASE}?page=3&q=oauth&label=label-1&sort=title:desc`);
    const url = applyTaskListUrlState(dirty, DEFAULT_TASK_LIST_URL_STATE);
    expect(url.search).toBe('');
  });

  it('扱わないクエリ（selected）は触らない', () => {
    const url = applyTaskListUrlState(new URL(`${BASE}?selected=ENG-42`), {
      ...DEFAULT_TASK_LIST_URL_STATE,
      page: 2,
    });
    expect(url.searchParams.get('selected')).toBe('ENG-42');
    expect(url.searchParams.get('page')).toBe('2');
  });

  it('書いた URL をそのまま読み戻せる', () => {
    const state = {
      page: 4,
      q: 'ログイン',
      labelId: 'label-9',
      sorting: [{ id: 'status', desc: false }],
    };
    const url = applyTaskListUrlState(new URL(BASE), state);
    const search = Object.fromEntries(url.searchParams.entries());
    expect(parseTaskListUrlState(search)).toEqual(state);
  });
});

describe('clampPage', () => {
  it('範囲内はそのまま', () => {
    expect(clampPage(2, 100, 20)).toBe(2);
    // 最終ページちょうど
    expect(clampPage(5, 100, 20)).toBe(5);
  });

  it('総件数より後ろのページは最終ページへ丸める', () => {
    expect(clampPage(9, 100, 20)).toBe(5);
    // 端数のある総件数
    expect(clampPage(9, 81, 20)).toBe(5);
  });

  it('0 件なら 1 ページ目', () => {
    expect(clampPage(3, 0, 20)).toBe(1);
  });
});

describe('useTaskListUrlSync', () => {
  function setup(initial: {
    href?: string;
    pageIndex?: number;
    q?: string;
    labelId?: string | null;
    sorting?: SortingState;
    total?: number;
    searching?: boolean;
  }) {
    window.history.replaceState({}, '', initial.href ?? '/acme/projects/ENG/tasks');
    const refs = {
      selectedTaskId: ref<string | null>(null),
      pagination: ref({ pageIndex: initial.pageIndex ?? 0, pageSize: 20 }),
      submittedSearchQuery: ref(initial.q ?? ''),
      selectedLabelId: ref<string | null>(initial.labelId ?? null),
      sorting: ref<SortingState>(initial.sorting ?? []),
      taskTotal: ref(initial.total ?? 0),
      isSearchActive: ref(initial.searching ?? false),
    };
    // watch を動かすためにコンポーネント文脈で呼ぶ
    const wrapper = mount(
      defineComponent({
        setup() {
          useTaskListUrlSync(refs);
          return () => null;
        },
      }),
    );
    return { refs, wrapper };
  }

  it('ページ送りが URL に載る（リロード・戻りで復元できる形になる）', async () => {
    const { refs } = setup({});

    refs.pagination.value = { pageIndex: 2, pageSize: 20 };
    await nextTick();

    expect(new URL(window.location.href).searchParams.get('page')).toBe('3');
  });

  it('検索語・ラベル・並び替え・選択をまとめて載せる', async () => {
    const { refs } = setup({});

    refs.submittedSearchQuery.value = 'oauth';
    refs.selectedLabelId.value = 'label-1';
    refs.sorting.value = [{ id: 'title', desc: true }];
    refs.selectedTaskId.value = 'ENG-42';
    await nextTick();

    const params = new URL(window.location.href).searchParams;
    expect(params.get('q')).toBe('oauth');
    expect(params.get('label')).toBe('label-1');
    expect(params.get('sort')).toBe('title:desc');
    expect(params.get('selected')).toBe('ENG-42');
  });

  it('履歴を積まない（戻るで一覧より前へ抜けられなくなるのを避ける）', async () => {
    const pushSpy = vi.spyOn(window.history, 'pushState');
    const { refs } = setup({});

    refs.pagination.value = { pageIndex: 1, pageSize: 20 };
    await nextTick();

    expect(pushSpy).not.toHaveBeenCalled();
    pushSpy.mockRestore();
  });

  it('件数が減って範囲外になったページは最終ページへ丸める', async () => {
    const { refs } = setup({ pageIndex: 8, total: 0 });

    refs.taskTotal.value = 100; // 20 件/頁 → 最終 5 頁
    await nextTick();

    expect(refs.pagination.value.pageIndex).toBe(4);
    expect(new URL(window.location.href).searchParams.get('page')).toBe('5');
  });

  it('範囲内のページは丸めない', async () => {
    const { refs } = setup({ pageIndex: 2, total: 0 });

    refs.taskTotal.value = 100;
    await nextTick();

    expect(refs.pagination.value.pageIndex).toBe(2);
  });

  it('検索中は丸めない（ページャを使わないため）', async () => {
    const { refs } = setup({ pageIndex: 8, total: 0, searching: true });

    refs.taskTotal.value = 3;
    await nextTick();

    expect(refs.pagination.value.pageIndex).toBe(8);
  });

  it('扱わないクエリは残す', async () => {
    const { refs } = setup({ href: '/acme/projects/ENG/tasks?ref=email' });

    refs.pagination.value = { pageIndex: 1, pageSize: 20 };
    await nextTick();

    const params = new URL(window.location.href).searchParams;
    expect(params.get('ref')).toBe('email');
    expect(params.get('page')).toBe('2');
  });
});
