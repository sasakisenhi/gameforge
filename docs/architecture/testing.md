# TDDとテスト戦略

> [アーキテクチャ概要](../../architecture.md)へ戻る

Red-Green-Refactor、TDD証拠、品質ゲート、層別テスト、実装順序を定義する。

## TDDとテスト戦略

### TDDを必須にする範囲

実行可能な振る舞いを追加・変更・修正するすべての作業で、TDDを必須とする。対象にはDomain、Application、Scheduler、Projector、Adapter、CLI、GUIのReducer/Component、競合解析、migration、Build処理を含む。

不具合修正では、報告された不具合を再現する失敗テストを最初に追加する。外部ツールやcrateのversion更新では、新versionのfixtureまたはPort contract testを先に追加・実行し、現在のAdapterでは満たせないこと、または既存契約が維持されることを確認してからAdapterを変更する。

例外は次に限定する。

- 実行結果へ影響しない文書だけの変更
- 最終成果へ取り込まない、明示的に破棄する調査用spike

spikeのコードを製品へ残す場合は例外を解除し、テストから実装し直す。緊急修正、自動修正、人間による手動調整もTDDを迂回する理由にしない。

### Red → Green → Refactor

一つの大きなTaskを一度だけRed/Greenにするのではなく、観測可能な最小の振る舞いごとに次のcycleを繰り返す。

```text
Acceptance Criterion
  → Testを先に追加
  → RED: 意図した理由で失敗することを確認
  → GREEN: 通すための最小限のproduction codeを実装
  → 全関連testを実行
  → REFACTOR: 重複・命名・境界を改善
  → GREENを再確認
  → 次の振る舞いへ
```

各phaseでは次を守る。

#### RED

- Acceptance Criterionまたは再現条件に対応するtest IDを定める。
- production codeを変更する前に、テストコード、fixture、test harnessだけを変更する。
- 対象testを実行し、未実装の振る舞いが原因で失敗することを確認する。
- compile errorをRedとする場合も、未定義APIなど意図した設計差分が原因でなければならない。
- 環境障害、fixture破損、無関係な既存testの失敗はRedの証拠にしない。
- 最初から成功するtestは、回帰防止にはなっても新しい振る舞いのRed証拠にはならない。

#### GREEN

- 確認済みの失敗を通す最小限のproduction codeだけを変更する。
- test側の期待値を実装へ合わせて弱めない。
- 対象testに加えて、影響範囲の既存testを実行する。
- flaky testの再実行成功をGreenとみなさず、非決定性の原因を解消する。

#### REFACTOR

- 外部から観測される振る舞いを変えず、構造だけを改善する。
- refactor前後で関連testがGreenであることを確認する。
- refactor中に新しい振る舞いが必要になった場合は、次のRed cycleへ分ける。

### Task ContractとTDD証拠

Task Contractには実装対象だけでなく、次のテスト情報を含める。

```yaml
test_plan:
  - test_id: TEST-AC-002-01
    acceptance_criterion: AC-002
    level: unit
    target: sand_stops_at_world_boundary
    command: cargo test -p game-logic sand_stops_at_world_boundary
    expected_red_reason: 境界判定が未実装のため砂が範囲外へ移動する
test_paths:
  - crates/game_logic/tests/**
```

`allowed_paths`には必要なtest pathを明示する。テストを書けないほどAcceptance Criterionが曖昧な場合、実装を開始せずDecision Requestを作成する。

Task Runはcycleごとに`TddCycleEvidence`を記録する。

```text
TddCycleEvidence {
  cycle_id,
  test_ids,
  acceptance_criterion_ids,
  red_revision,
  red_command,
  red_result_digest,
  red_failure_reason,
  first_production_revision,
  green_revision,
  green_command,
  green_result_digest,
  refactor_revision,
  final_suite_result
}
```

ここでrevisionは必ずしもGit commitを要求せず、commit SHAまたはControl Planeが記録したcontent-addressed worktree snapshotを使用する。RedとGreenの間で、どのtestとproduction fileがいつ変わったかを後から検証可能にする。

Event Journalには少なくとも`TestSpecified`、`RedConfirmed`、`ProductionChangeStarted`、`GreenConfirmed`、`RefactorConfirmed`を順番に記録する。Red確認前にproduction pathの変更を観測した場合は`TDD_SEQUENCE_VIOLATION`を付け、Integration Candidateへの統合を停止する。

Red phaseで期待されるテスト失敗はTask Runの`FAILED`ではなく、正常な開発証拠として扱う。ただしRedのままcycleを終了した場合や、最終suiteがGreenでない場合は`SUCCEEDED`へ遷移できない。

TDD phaseはTask Runの主状態と直交する補助状態として管理する。

```text
TEST_PLANNED
  → RED_WRITING
  → RED_CONFIRMED
  → GREEN_IMPLEMENTING
  → GREEN_CONFIRMED
  → REFACTORING
  → CYCLE_COMPLETED
```

Control Planeは同じCodex threadを継続利用できるが、原則としてRed、Green、Refactorを別turnとして実行する。Red turn完了時にテスト失敗の理由とsnapshotを検証してから、Green turnを開始する。

Red phaseでは、可能な実行sandboxでtest path、fixture path、明示的に承認したtest harness/manifest pathだけを書き込み可能にする。Rustの`#[cfg(test)]` moduleのようにproduction codeとtestが同一ファイルにある場合は、ASTまたはdiff hunkでtest領域以外が変わっていないことを検査する。これを確実に検査できない場合は、別の`tests/` fileへtestを置く。Red確認後にだけproduction pathへの書き込みを解放する。

人間がZedで修正する場合も同じphaseと検査を適用する。Red確認前にproduction変更が必要だと判明した場合は、変更を進めずtest seamまたはAPI設計についてDecision Requestを作成する。

### TDD品質ゲート

Local Checkは最終的なテスト成功だけでなく、次を検証する。

- 各Acceptance Criterionに少なくとも一つのtestまたは明示的な人間のplaytest scenarioが対応している。
- 自動テスト可能な振る舞いをplaytestだけで代替していない。
- production diffより前に対応するRed証拠がある。
- RedとGreenが同じtest IDと期待する振る舞いを対象にしている。
- testの削除、skip追加、assertion削減、coverage対象除外に承認のない弱体化がない。
- 最終revisionで対象test、crate test、必要なworkspace testがGreenである。
- testが時刻、ID、外部I/O、実行順に不必要に依存せず、再現可能である。

CIは最終revisionのGreenと回帰がないことを再検証する。Red→Greenの時系列そのものは、Task RunのEvent Journal、revision、実行結果digestを正本とする。独立AI Reviewは、テストが実装詳細ではなく要求された振る舞いを検証しているか、重要な境界条件が不足していないかを確認する。

### Domain / Application / Scheduler

実装前に次のtestを書く。

- 状態遷移表に基づくunit test
- 不正遷移と不変条件のproperty test
- Use Caseとfake PortによるCommand/Eventのscenario test
- Task DAGの循環検出、topological order、critical path
- commit変更時のEvidence無効化
- Schedulerの決定性、容量制約、競合時の直列化
- path正規化とscope判定
- Clock、ID、外部結果をfake化した再現可能な失敗・復旧test

### Analyzer / Projection

実装前に、入力と期待recordまたはProjectionをfixtureとして固定する。

- Rust構文fixtureによるSymbolDeltaのgolden test
- 各Conflict Ruleのtable-driven test
- 未対応構文が`UNKNOWN`になることのtest
- Event再適用の冪等性
- 期待するEvent列から画面Query DTOへのProjection test
- 空SQLiteからのfull rebuildと、逐次Projectionの一致
- migration前後で同じ論理Query結果になることのtest

### Adapters

外部ツールを呼ぶ実装より先に、Port contractとwire fixtureを用意する。

- Port contract testをfakeと実Adapterの両方へ適用する。
- 外部tool/versionごとのwire fixtureをMapperへ再生し、内部DTOが安定していることを確認する。
- 外部crate更新前後で同じPort contract suiteを実行し、変更がAdapter内に閉じていることを確認する。
- 一時障害、process終了、認証失敗、protocol version差のintegration testを行う。
- Git adapterは一時repository/worktreeを使い、HEADと未記録変更を検証する。
- 外部APIのテストでは録画済みfixtureまたはsandboxを使い、Domain testからnetworkへ接続しない。

### GUI / End-to-End

画面実装より先に、View DTOとUser Intentに対する期待表示・Commandをtestへ記述する。

- ReducerとCommand生成をpure testする。
- 主要画面を固定Read Modelでcomponent testする。
- loading、empty、stale、error、approval待ちを状態ごとにtestする。
- `MVP.md`の完成シナリオを、fake Adapterから始めて実Adapterへ段階的に置き換える。
- 最終的にFirst Playableのcommit生成、検証、playtest記録、mergeまでを通す。

### Coordinatorとprocess競合

一時project directoryと複数OS processを使うintegration testを、Coordinator実装より先に用意する。

- Desktop役とCLI役が同時にlease取得を試みても、成功するCoordinatorが一つだけである。
- lease取得に失敗したClientがEvent Journal、SQLite write connection、具体Adapterを開かない。
- DesktopとCLIが同じAggregateへ同時Commandを送った場合、version順に一つだけ受理される。
- 同じ`command_id`をtimeout後に再送しても、Domain Eventと外部Operationが重複しない。
- CoordinatorをEvent append、Projection、外部Operationの各境界で強制終了し、次のCoordinatorがReconciliationできる。
- 古いendpoint metadataが残りlockが解放済みの場合は安全に復旧し、lockが生きている場合は奪取しない。
- protocol versionが非互換なClientからのMutationを拒否する。
- 異なるproject rootでは、それぞれ一つのCoordinatorを並行起動できる。

## 実装順序

縦切りを優先し、各段階でDomain、Event、Projection、最小UI/CLIを一緒に通す。

以下の各項目はproduction codeの実装順ではなく、必ず「失敗するtest/fixtureを追加 → Red確認 → 最小実装 → Green確認 → Refactor」の順で進める。

1. `Task`、`TaskRun`、`Build`、`Acceptance`の遷移testを書き、ID、状態機械、不変条件を実装する。
2. 文書・Event fixtureとrebuild testを書き、Markdown loader、Event Journal、SQLiteの最小経路を実装する。
3. 複数processのlease競合testを書き、ProjectCoordinator、control protocol、共通bootstrapを実装する。
4. CLI Clientのscenario testを書き、Coordinator経由のTask検証と状態照会を通す。
5. Component testを書き、Dioxus Clientから同じCommandとQueryを呼ぶ。
6. Scheduler scenario testとfake Adapterを書き、QueueからBoard更新までを通す。
7. Port contractと異常系testを書き、Git、Codexの実Adapterを接続する。
8. Conflict Ruleのfixture testを書き、ファイル競合からAST競合へ拡張する。
9. verification/buildのcontract testを書き、GitHub、AI Review、Build Adapterを接続する。
10. commit不一致を拒否するend-to-end testを書き、Acceptanceとmerge gateを完成させる。

初期段階から全テーブルや全Contextを作り切らず、「一つのTaskをQueueし、実行結果を記録し、Boardへ表示する」最小の縦切りで境界を検証する。

