-- Drive の階層と project_id の食い違いを直す backfill。
--
-- 修正前の create_folder は親の project_id を継承せず、upload_file はフォルダの
-- project_id（= NULL）をそのまま入れていた。そのためプロジェクトフォルダ配下に
-- 作られた子フォルダとその中のファイルが project_id = NULL のまま残り、配信の認可
-- （drive_files.project_id で判定する）が「一般ファイル」と読んで非メンバーにも見える。
-- 作成・移動の側は直したが、既に食い違っている行はそのままなので、ここで揃える。
--
-- プロジェクトルート（project_id IS NOT NULL AND parent_id IS NULL）を起点に子孫を
-- 辿り、フォルダとファイルの project_id をルートの値へ更新する。意味は
-- service::drive::sync_subtree_project_id と同じで、対象がプロジェクトルート全件に
-- なるだけ。
--
-- **ただし、配下に別プロジェクトの行があるツリーは一切書き換えず、失敗させる。**
-- 修正前はプロジェクトルートの移動も移動先 ACL の無視もできたので、
-- 「A のルートが B のツリー配下にある」状態を作れた。親の値をそのまま伝播すると
-- A のルートと配下のファイルが B のものになり、A のファイルが B のメンバーへ公開され、
-- A のメンバーはアクセスを失う。直すための backfill が新しい漏れを作ってはいけないので、
-- 継承値と食い違う非 NULL の project_id を見つけたら止めて、人が直してから流し直す。
--
-- **逆向き（一般ツリーの配下に project_id を持つ行）は触らない。** そちらは階層より
-- 厳しい判定になるだけで、漏れる向きではない。NULL へ落とすと非メンバーへ開いてしまう。
--
-- folder_id が NULL のファイル（ドライブ直下）も対象外。属するフォルダが無いので
-- 階層から project_id を導けない。

-- 1) 境界の食い違いを検出する。1 件でもあれば例外で止める（更新は次の文なので、
--    ここで止まれば何も書き換わらない）
DO $$
DECLARE
    conflict_count bigint;
    sample text;
BEGIN
    WITH RECURSIVE subtree AS (
        SELECT id, project_id, 1 AS depth
          FROM drive_folders
         WHERE project_id IS NOT NULL
           AND parent_id IS NULL
        UNION ALL
        SELECT child.id, parent.project_id, parent.depth + 1
          FROM drive_folders child
          JOIN subtree parent ON child.parent_id = parent.id
         -- 循環は validate_parent_folder が作らせないが、万一混ざっても止まるよう深さで切る
         -- （実際の階層は 3〜5 段）
         WHERE parent.depth < 64
    )
    SELECT count(*), min(f.id::text)
      INTO conflict_count, sample
      FROM drive_folders AS f
      JOIN subtree ON f.id = subtree.id
     WHERE f.project_id IS NOT NULL
       AND f.project_id IS DISTINCT FROM subtree.project_id;

    IF conflict_count > 0 THEN
        RAISE EXCEPTION
            'drive_folders に別プロジェクトのツリーが入れ子になっています（% 件、例: %）。'
            '継承で上書きすると、そのプロジェクトのファイルが別プロジェクトのメンバーへ '
            '公開されます。該当フォルダを正しい親へ戻すか project_id を直してから、'
            'このマイグレーションを流し直してください。',
            conflict_count, sample;
    END IF;
END
$$;

-- 2) フォルダ
WITH RECURSIVE subtree AS (
    SELECT id, project_id, 1 AS depth
      FROM drive_folders
     WHERE project_id IS NOT NULL
       AND parent_id IS NULL
    UNION ALL
    SELECT child.id, parent.project_id, parent.depth + 1
      FROM drive_folders child
      JOIN subtree parent ON child.parent_id = parent.id
     WHERE parent.depth < 64
)
UPDATE drive_folders AS f
   SET project_id = subtree.project_id
  FROM subtree
 WHERE f.id = subtree.id
   AND f.project_id IS DISTINCT FROM subtree.project_id;

-- 3) ファイル。属するフォルダの project_id へ揃える
--    （フォルダ側は 2) で揃っているので、食い違う非 NULL は残っていない）
WITH RECURSIVE subtree AS (
    SELECT id, project_id, 1 AS depth
      FROM drive_folders
     WHERE project_id IS NOT NULL
       AND parent_id IS NULL
    UNION ALL
    SELECT child.id, parent.project_id, parent.depth + 1
      FROM drive_folders child
      JOIN subtree parent ON child.parent_id = parent.id
     WHERE parent.depth < 64
)
UPDATE drive_files AS fi
   SET project_id = subtree.project_id
  FROM subtree
 WHERE fi.folder_id = subtree.id
   AND fi.project_id IS DISTINCT FROM subtree.project_id;
