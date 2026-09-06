-- Drive の階層と project_id の食い違いを直す backfill。
--
-- 修正前の create_folder は親の project_id を継承せず、upload_file はフォルダの
-- project_id（= NULL）をそのまま入れていた。そのためプロジェクトフォルダ配下に
-- 作られた子フォルダとその中のファイルが project_id = NULL のまま残り、配信の認可
-- （drive_files.project_id で判定する）が「一般ファイル」と読んで非メンバーにも見える。
-- 作成・移動の側は直したが、既に食い違っている行はそのままなので、ここで揃える。
--
-- project_id を持つフォルダを起点に、project_id が NULL の子孫だけを辿り、そのフォルダと
-- 中のファイルへ起点の値を入れる。意味は service::drive::sync_subtree_project_id と同じで、
-- 対象が「project_id を持つフォルダ」全件になるだけ。
--
-- **起点をドライブ直下（parent_id IS NULL）に限らない。** 修正前はプロジェクトルートを
-- 一般フォルダ配下へ移動できたので、親を持つプロジェクトルートが残りうる。ドライブ直下の
-- ものだけを起点にすると、そのルート配下の NULL の子孫が修復されず、プロジェクト非メンバー
-- から見えたままになる。
--
-- **配下に別プロジェクトの行があるツリーは一切書き換えず、失敗させる。**
-- 修正前はプロジェクトルートの移動も移動先 ACL の無視もできたので、
-- 「A のルートが B のツリー配下にある」状態を作れた。親の値をそのまま伝播すると
-- A のルートと配下のファイルが B のものになり、A のファイルが B のメンバーへ公開され、
-- A のメンバーはアクセスを失う。直すための backfill が新しい漏れを作ってはいけないので、
-- 継承値と食い違う非 NULL の project_id を見つけたら止めて、人が直してから流し直す。
-- フォルダだけでなくファイルも同じ扱いにする（黙って上書きすると同じ向きの漏れになる）。
--
-- **逆向き（一般ツリーの配下に project_id を持つ行）は触らない。** そちらは階層より
-- 厳しい判定になるだけで、漏れる向きではない。NULL へ落とすと非メンバーへ開いてしまう。
--
-- folder_id が NULL のファイル（ドライブ直下）も対象外。属するフォルダが無いので
-- 階層から project_id を導けない。
--
-- 階層の深さでは打ち切らない。API に深さの上限は無く、途中で止めると残りが NULL のまま
-- 「成功」してしまう。循環は validate_parent_folder が作らせないが、万一混ざっていたら
-- 更新せず失敗させる。
--
-- **循環の検出は伝播用の CTE とは別に行う。** 伝播側は project_id が NULL の子だけを
-- 辿るので、循環して起点へ戻る最後の一歩（起点は非 NULL）を踏めず、CYCLE 句が立たない。
-- 全フォルダを起点に parent_id を親方向へ辿る CTE を別に置き、そちらで検出する。
-- 全ノードが NULL の循環も、循環内のフォルダ自身が起点になるので拾える。

-- 1) 循環と境界の食い違いを検出する。1 件でもあれば例外で止める（更新は次の文なので、
--    ここで止まれば何も書き換わらない）
DO $$
DECLARE
    cycle_count bigint;
    cycle_sample text;
    conflict_count bigint;
    conflict_sample text;
BEGIN
    WITH RECURSIVE ancestors AS (
        -- 循環の検出。全フォルダを起点に親方向へ辿る
        SELECT id AS start_id, id, parent_id
          FROM drive_folders
        UNION ALL
        SELECT ancestors.start_id, parent.id, parent.parent_id
          FROM drive_folders AS parent
          JOIN ancestors ON ancestors.parent_id = parent.id
    ) CYCLE id SET is_cycle USING path,
    subtree AS (
        SELECT id, project_id
          FROM drive_folders
         WHERE project_id IS NOT NULL
        UNION ALL
        -- project_id を持つ子は自分が起点になるので辿らない。ここで辿るのは
        -- 継承先（NULL）だけで、食い違いの検出は下の problems で別に行う
        SELECT child.id, parent.project_id
          FROM drive_folders child
          JOIN subtree parent ON child.parent_id = parent.id
         WHERE child.project_id IS NULL
    ) CYCLE id SET is_cycle USING path,
    problems AS (
        -- is_cycle が立った行の id は、経路に 2 度出てきたフォルダ（＝循環の一部）
        SELECT DISTINCT 'cycle' AS kind, id
          FROM ancestors
         WHERE is_cycle
        UNION ALL
        SELECT 'drive_folders', child.id
          FROM drive_folders AS child
          JOIN subtree AS parent ON child.parent_id = parent.id
         WHERE child.project_id IS NOT NULL
           AND child.project_id IS DISTINCT FROM parent.project_id
        UNION ALL
        SELECT 'drive_files', fi.id
          FROM drive_files AS fi
          JOIN subtree ON fi.folder_id = subtree.id
         WHERE fi.project_id IS NOT NULL
           AND fi.project_id IS DISTINCT FROM subtree.project_id
    )
    SELECT count(*) FILTER (WHERE kind = 'cycle'),
           min(id::text) FILTER (WHERE kind = 'cycle'),
           count(*) FILTER (WHERE kind <> 'cycle'),
           min(kind || ' ' || id::text) FILTER (WHERE kind <> 'cycle')
      INTO cycle_count, cycle_sample, conflict_count, conflict_sample
      FROM problems;

    IF cycle_count > 0 THEN
        RAISE EXCEPTION
            'drive_folders の親子関係が循環しています（% 件、例: %）。'
            '循環したツリーは project_id を継承できないので、親を直してから、'
            'このマイグレーションを流し直してください。',
            cycle_count, cycle_sample;
    END IF;

    IF conflict_count > 0 THEN
        RAISE EXCEPTION
            'Drive に別プロジェクトの行が入れ子になっています（% 件、例: %）。'
            '継承で上書きすると、そのプロジェクトのファイルが別プロジェクトのメンバーへ '
            '公開されます。該当する行を正しい親へ戻すか project_id を直してから、'
            'このマイグレーションを流し直してください。',
            conflict_count, conflict_sample;
    END IF;
END
$$;

-- 2) フォルダ。継承先（project_id が NULL）だけを埋める
WITH RECURSIVE subtree AS (
    SELECT id, project_id
      FROM drive_folders
     WHERE project_id IS NOT NULL
    UNION ALL
    SELECT child.id, parent.project_id
      FROM drive_folders child
      JOIN subtree parent ON child.parent_id = parent.id
     WHERE child.project_id IS NULL
) CYCLE id SET is_cycle USING path
UPDATE drive_folders AS f
   SET project_id = subtree.project_id
  FROM subtree
 WHERE f.id = subtree.id
   AND f.project_id IS NULL;

-- 3) ファイル。属するフォルダの project_id へ揃える
--    （食い違う非 NULL は 1) で止めているので、残っているのは NULL だけ）
WITH RECURSIVE subtree AS (
    SELECT id, project_id
      FROM drive_folders
     WHERE project_id IS NOT NULL
    UNION ALL
    SELECT child.id, parent.project_id
      FROM drive_folders child
      JOIN subtree parent ON child.parent_id = parent.id
     WHERE child.project_id IS NULL
) CYCLE id SET is_cycle USING path
UPDATE drive_files AS fi
   SET project_id = subtree.project_id
  FROM subtree
 WHERE fi.folder_id = subtree.id
   AND fi.project_id IS NULL;
