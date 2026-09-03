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
-- **逆向き（一般ツリーの配下に project_id を持つ行）は触らない。** そちらは階層より
-- 厳しい判定になるだけで、漏れる向きではない。NULL へ落とすと非メンバーへ開いてしまう。
--
-- folder_id が NULL のファイル（ドライブ直下）も対象外。属するフォルダが無いので
-- 階層から project_id を導けない。

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
UPDATE drive_folders AS f
   SET project_id = subtree.project_id
  FROM subtree
 WHERE f.id = subtree.id
   AND f.project_id IS DISTINCT FROM subtree.project_id;

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
