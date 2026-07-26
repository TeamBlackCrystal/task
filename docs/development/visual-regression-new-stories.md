# 新しい Storybook story の視覚テスト運用

このガイドは、新しい story ファイルまたは既存ファイルの story entry を追加する開発者を対象とする。

選択処理の仕組みとプロバイダを載せ替えるときの境界は、[視覚テストの差分選択設計](visual-regression-selection-design.md)に記載している。

## Pull Request で起きること

新しい story は、比較対象の baseline に同名の screenshot がないため、Argos では added と表示される。

added は撮影や比較の失敗を意味しない。
「baseline に同名の screenshot が存在しない」という比較結果である。

新しい story ファイルを追加すると、selector は変更された story ファイルを preview graph の seed にする。
そのファイルに属する `type=story` の entry を `selected_story_ids` へ入れ、PARTIAL run として撮影する。

既存の story ファイルに entry を追加した場合も、その entry は独立した story ID として manifest、実行記録、撮影記録に現れる。
ただし、selector はソース差分を entry の構文単位では解析しない。
変更されたファイルに属する全 story entry を選ぶため、「追加した entry だけを撮影する」という意味での entry 単位選択ではない。

docs entry は `type=docs` なので選択対象に入らない。
同じ import path に docs entry があっても、story ID の集合には混ざらない。

## PARTIAL run と baseline

PARTIAL run は Argos CLI の `--subset` として upload する。

Argos の baseline 候補検索は `subset=true` の build を明示的に除外する。
したがって、PARTIAL run を承認しても、その build が将来の baseline へ昇格することはない。

新しい story は、次の条件をすべて満たす非 subset build で初めて baseline 候補に入る。

- build が完了している。
- framework test が成功し、screenshot bucket が valid である。
- build が rejected ではない。
- reference build、orphan build、承認済みの check build、または merge 済み Pull Request の check build である。

現在の workflow では、main への push が全 story を撮る FULL backstop であり、main の reference build は自動承認される。
そのため、Pull Request の PARTIAL run で added を確認し、変更が main へ入った後の FULL run が新しい story を含む baseline 候補になる。

## 開発者が確認すること

Argos の added screenshot が意図した初期表示であることを確認する。
意図した表示なら、通常の visual review と同じように承認する。

この承認は Pull Request の review 結果を確定する操作であり、PARTIAL build を baseline へ昇格させる操作ではない。

意図しない表示なら、story、fixture、mock、時刻固定を修正してから再実行する。
added であることだけを理由に失敗として扱ったり、baseline 不在を隠すために既存 screenshot 名を流用したりしない。

## 撮影されなかったときの調査

GitHub Actions の `vrt-selection-<run id>-<attempt>` artifact を取得し、次の順序で確認する。

1. `.vrt/selection-manifest.json` の `changed_paths` と `in_scope_paths` に story ファイルがあるか確認する。
2. `mode`、`reason_codes`、`baseline_commit`、`head_commit` を確認する。
3. `selected_story_ids` に追加した story ID があるか確認する。
4. `.vrt/executed-story-ids.txt` と `.vrt/captured-story-ids.txt` に同じ ID があるか確認する。
5. `.vrt/capture-gate.json` の `pending`、`skipped`、`failed`、`screenshots_outside_manifest` がすべて 0 か確認する。
6. `source_index_sha256`、`filtered_index_sha256`、`served_index_sha256` の関係を確認する。

PARTIAL の gate は次の不変条件を要求する。

```text
selected_story_ids == executed_story_ids == captured_story_ids
```

新しい story に baseline がなくても、この三集合が一致し、served index の SHA が manifest と一致すれば gate は通る。
gate は visual baseline の有無を合否条件にしていないため、Argos 側では upload 後に added と判定できる。

`selected_story_ids` に ID がなければ、Storybook build の `index.json` で entry の `type`、`id`、`importPath` を確認する。
`type=docs` の entry は仕様どおり除外される。

次に `.vrt/preview-graph.json` で `importPath` に対応する story module を確認する。
preview graph は Storybook の Vite build が module graph を解決した後に生成されるため、新しいファイルが Storybook の `stories` glob に一致しなければ graph に載らない。

graph の seed が解決できない場合、selector は撮影漏れを避けるため FULL へ退避し、`preflight_fail_closed` または `zero_reach_fail_closed` を記録する。
この場合は reason を消すために manifest を手で書き換えず、Storybook の glob、ファイル拡張子、import path、graph の正規化を修正する。

SHA が一致しない場合、selector が作った filtered index と、test runner が実際に配信された index が異なる。
古い `storybook-static` の再利用、selector 実行後の index 書換え、別ディレクトリの配信を疑う。

## 現在の検出限界

selector が直接 seed にできるのは、`apps/frontend/` 以下の Vue、JavaScript、TypeScript module である。
CSS、Storybook 設定、lockfile、workflow などの変更は影響範囲を安全に限定できないため FULL になる。

MDX は docs 用 glob に含まれるが、selector の story module seed には含まれない。
MDX 変更で visual story を追加する運用を導入する場合は、index と graph の対応を fixture で先に証明してから selector の対象を拡張する必要がある。

rename と copy は変更前後のパスを差分集合へ入れる。
変更前の module が現行 graph に存在しない場合は解決失敗として FULL へ退避するため、撮影漏れにはならない。
