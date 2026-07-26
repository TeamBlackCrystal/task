# 視覚テストの差分選択設計

この文書は、視覚テストの撮影集合を変更差分から決める contract と、visual testing provider を載せ替えるときの境界を定める。

新しい story を追加するときの開発者向け手順は、[新しい Storybook story の視覚テスト運用](visual-regression-new-stories.md)に記載している。

## 設計上の不変条件

視覚差分がないことを selector 自体は保証しない。
selector が保証するのは、provider が実際に比較する baseline commit から HEAD までの変更を入力にし、選んだ story を過不足なく実行して撮影したことだけである。

撮影工程は次の不変条件を満たさなければ upload へ進めない。

```text
selected_story_ids == executed_story_ids == captured_story_ids
pending == skipped == failed == screenshots_outside_manifest == 0
served_index_sha256 == expected_index_sha256
```

不明な module、壊れた graph、未確認の baseline を「影響なし」とみなさない。
影響範囲を限定できない入力は FULL へ退避する。

PARTIAL build を baseline 候補にしない。
部分集合を完全な screenshot 集合として再利用すると、選ばれなかった story の差分が将来の比較から消えるためである。

## 入出力 contract

**provider-neutral selector** は `.github/scripts/vrt-selector.mts` に置く。
selector は provider の build ID、review payload、API response を受け取らない。

selector の入力は次の値である。

- provider が選んだ `baselineCommit`。確認できない場合は `null`。
- `headCommit`。
- baseline から HEAD までの rename と copy の両側を含む `changedPaths`。
- main、dependency update、dependency phase の分類。
- Storybook の `index.json`。
- Storybook build が生成した preview graph。
- adapter が検出した preflight error。

selector の出力は selection manifest と、PARTIAL の場合だけ filtered Storybook index である。
manifest は mode、commit、変更パス、story ID、reason code、source index、filtered index、graph の SHA-256 を持つ。

SHA は安定順序の JSON から計算する。
artifact の内容と gate が実際に配信した index を同じ値で照合するためである。

## preview graph

`apps/frontend/buildSrc/vrtGraphPlugin.ts` は Storybook の Vite build 内で `getModuleIds` と `getModuleInfo` を読み、schema version 1 の graph を生成する。

各 module の `reasons` は、その module が import する静的 module と動的 module である。
selector は辺を逆向きにたどり、変更 module を import する story module を完全 BFS で求める。

新しい story ファイルは Storybook build が index と graph を作った後に selector へ渡る。
selector は `apps/frontend/` 以下の Vue、JavaScript、TypeScript module を seed にできるため、`apps/frontend/stories/` の追加ファイルも graph から選べる。

index は `type=story` だけを正規化する。
docs entry を同じ import path の story と混同しない。

同じ story ファイルに複数 entry がある場合、各 entry は別の story ID として manifest に入る。
ただし、変更差分を story export の構文単位には分解せず、変更されたファイルに属する全 story ID を選ぶ。

## FULL、PARTIAL、NONE

**FULL** は全 story を撮影する。
main の per-merge backstop、baseline 未確認、preflight error、denylist、zero reach、graph 解決失敗で選ぶ。

**PARTIAL** は graph から到達した story だけを撮影する。
filtered index を Storybook の配信対象へ置き換え、manifest と filtered index の SHA を gate で照合する。

**NONE** は視覚テストの対象外であることを証跡化し、撮影と upload を行わない。
変更パスがすべて対象外の場合、または dependency graph が phase 1 で到達 story 0 と証明した場合に選ぶ。

NONE でも manifest と gate artifact を残す。
「workflow が動かなかった」のか「対象外と判定した」のかを区別するためである。

## baseline と変更パス

Argos adapter は `/v2/baseline` へ HEAD の ancestor commit を近い順に渡し、Argos が選んだ eligible build の HEAD SHA を `baselineCommit` とする。

baseline 候補は complete、valid、非 subset、非 skipped、非 rejected でなければならない。
reference または orphan は承認済みとして扱い、check は承認済み review または merge 済み Pull Request を必要とする。

Argos の実 build 44 では、HEAD `5313b21ea28a5cb2b73514d3d5624550a07d8d11` に対し、build 43 の `18e85b91d2916d75fa9114e8dc651c6536eddf42` が `baseBuild.head.sha` と `baseScreenshotBucket.commit` の両方に現れた。
adapter が利用する baseline API と Argos build の公開情報が同じ commit を示すことを確認した。

変更パスは baseline を B、HEAD を H とする B..H の commit 履歴から集める。
rename と copy は変更前後の両方を含める。
Pull Request の base SHA だけを使わないのは、stacked Pull Request や main の build gap で provider の baseline が base SHA より古くなる場合があるためである。

## fail-closed 分類

次の変更は graph の到達集合だけでは安全に限定できないため FULL にする。

- lockfile、workspace 定義、frontend package 定義。
- Storybook 設定、provider workflow。
- CSS、SCSS、Sass、Less。
- icon と virtual module。
- graph schema 不正、未解決 seed、未解決 reason。

story module の追加や変更は graph で解決できるため PARTIAL にできる。
解決できなければ `preflight_fail_closed` または `zero_reach_fail_closed` で FULL に戻る。

dependency update は phase 0 を FULL とする。
phase 1 では package owner と graph 到達集合を使い、到達 story があれば PARTIAL、0 と証明できれば NONE にできる。
pnpm module ID の package owner は、二つ目の `node_modules` より下の package 名から求める。

## capture と証跡

test runner は story の訪問前に `.vrt/executed-story-ids.txt` へ ID を記録し、screenshot 完了後に `.vrt/captured-story-ids.txt` へ同じ ID を記録する。

gate は test result、served index、manifest、二つの ID 集合を読み、exact-set と SHA を検証する。
一つでも一致しなければ upload を止める。

workflow は PARTIAL のときだけ provider adapter へ subset を伝える。
Argos では CLI の `--subset` がこの役割を持つ。

selection manifest、source index、graph、実行ID、撮影ID、test result、gate result は Actions artifact として残す。
独立した品質確認では、被検証者が作った集計値を信用せず、同じ commit から fixture と selector を再実行して集合と SHA を再生成する。

## provider の載せ替え

載せ替え時に再利用する範囲は次のとおりである。

- Storybook index の正規化と docs 除外。
- preview graph の生成と完全 BFS。
- FULL、PARTIAL、NONE の分類。
- selection manifest と SHA。
- filtered index。
- executed、captured、served index の exact gate。
- fail-closed の denylist と reason code。

書き換える範囲は provider adapter と upload step である。

- provider が実際に選ぶ baseline commit の取得。
- eligible baseline の条件確認。
- subset build を baseline 候補から除外する方法。
- reference branch と review の承認規則。
- provider 固有の認証、build name、upload option。
- provider UI の added、changed、removed の表示と review 操作。

載せ替え先が「比較に使った baseline commit」を取得できなければ、PARTIAL を有効にしない。
PR base や latest main を代用品にすると、provider の実 baseline との gap にある変更を選択集合から落とす可能性があるためである。

載せ替え先が subset build の baseline 除外を保証できなければ、PARTIAL upload を行わない。
この条件を満たせない provider では FULL のみを使うか、完全集合を保持する自前の baseline store が必要になる。
