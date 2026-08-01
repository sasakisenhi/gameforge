# AI並列ゲーム開発コントロールプレーン アーキテクチャ

> Status: Draft / たたき台  
> Based on: `MVP.md` v0.1  
> Scope: MVPの全体アーキテクチャと設計文書への入口

## 1. この文書の役割

本書は、システム全体の境界、依存方向、正本、主要な不変条件を短く示す。実装に必要な詳細は、責務ごとに分割した設計文書を参照する。

本システムのアーキテクチャを一文で表すと、次のとおりである。

> 制作意図・実行・検証・受け入れをドメインとしてモデル化し、純粋な判定ロジックを中心に置き、Git、Codex、GitHub、SQLite、DioxusをPorts and Adaptersで接続するローカルファーストのコントロールプレーン。

## 2. 設計文書

| 文書 | 主な内容 |
| --- | --- |
| [中核ドメインモデル](domain-model.md) | Task、Task Run、Integration Candidate、Build、Acceptance Resultの関係と不変条件 |
| [GUI設計](ui-design.md) | 情報設計、画面構成、操作、状態表現、視覚・アクセシビリティ要件 |
| [ドメイン設計](docs/architecture/domain.md) | Bounded Context、Aggregate、状態機械、不変条件、Functional Core |
| [データ、正本、整合性](docs/architecture/data-and-consistency.md) | Markdown、Event Journal、SQLite Projection、AST競合解析 |
| [Applicationと外部連携](docs/architecture/application-and-integrations.md) | Use Case、Ports and Adapters、外部依存の局所化、技術スタック、crate依存 |
| [Runtime、Scheduler、GUI](docs/architecture/runtime-and-ui.md) | Scheduler、Dioxus、Tokio、ProjectCoordinator、単一Writer、Desktop/CLI競合 |
| [TDDとテスト戦略](docs/architecture/testing.md) | Red-Green-Refactor、TDD証拠、品質ゲート、実装順序 |
| [運用、監査、設計判断](docs/architecture/operations.md) | エラー、監査、セキュリティ、ADR候補、完成条件 |

要件とMVP完成シナリオは[`MVP.md`](MVP.md)を正とする。本書と詳細設計文書は、MVPを実装可能な境界と規則へ落とし込む。

## 3. 設計原則

各設計手法をシステム全体へ一律に適用せず、解決する問題に応じて使い分ける。

| 対象 | 主な設計手法 | 適用方針 |
| --- | --- | --- |
| システム全体の概念整理 | ドメイン駆動設計 | 制作意図、実行、検証、受け入れの言葉と境界を定義する |
| 状態機械・不変条件・判定 | 関数型プログラミング | 不変データと純粋関数で遷移・可否・無効化を判定する |
| 外部連携 | Ports and Adapters | 能力をPortとして定義し、外部固有型をAdapter内へ閉じ込める |
| SQLite Read Model | データ指向 | 画面ごとの問い合わせに適したProjectionとして保持する |
| AST・競合解析 | 関数型＋データ指向 | 不変な解析snapshotを変換し、集合演算で競合を判定する |
| Dioxus GUI | 状態駆動・関数型UI | Read Modelを描画し、User IntentをCommandへ変換する |
| Scheduler | Functional Core / Imperative Shell | 開始可否の判定とprocess・resource管理を分離する |
| 開発プロセス | Test-Driven Development | Red、Green、Refactorの順序をTask Runの証拠として残す |

横断的に次を守る。

- DomainはDioxus、Tokio、SQLite、Git、Codex、GitHub APIへ依存しない。
- GUIとCLIは同じUse Case、Command、Queryを使用する。
- 一つのproject rootでMutationできる`ProjectCoordinator`は同時に一つだけとする。
- 外部ツール固有型をDomain、Application Port、View DTOへ漏らさない。
- AIの推測ではなく、保存された事実から状態を導出する。
- `UNKNOWN`、古いEvidence、commit不一致を成功として扱わない。
- SQLiteは正本にせず、MarkdownとEvent Journalから再構築可能にする。
- production codeより先に、意図した理由で失敗するtestを作る。
- 自動化不能な判断は、理由と影響をHuman Inboxへ送る。

## 4. システムコンテキスト

```text
                              ┌──────────────┐
                              │    Human     │
                              └──────┬───────┘
                                     │ 意図・承認・プレイ判断
                              ┌──────▼───────┐
                              │ Desktop / CLI│
                              │   Clients    │
                              └──────┬───────┘
                                     │ Local Command / Query
                           ┌─────────▼──────────┐
                           │ ProjectCoordinator │
                           │  one per project   │
                           └─────────┬──────────┘
                                     │ Use Case / Port
┌─────────┐  JSON-RPC  ┌─────────────▼─────┐  CLI/filesystem  ┌──────────┐
│ Codex   │◀──────────▶│   Control Plane   │◀───────────────▶│ Git/Zed  │
└─────────┘             └─────────────┬─────┘                 └──────────┘
                                     │ API / checks / PR
                              ┌──────▼───────┐
                              │   GitHub     │
                              └──────────────┘
```

本システムは意図、Task、実行、Evidence、Build、Acceptanceの関連と状態遷移を所有する。Git、Codex、GitHub、Zed、Artifact Storeが所有する実体は、安定したID、commit、hash、最後に観測した状態で参照する。

## 5. 論理アーキテクチャ

```text
Desktop / CLI Clients
          │ Command / Query
          ▼
ProjectCoordinator ─── Scheduler / Runtime resources
          │
          ▼
Application Use Cases ─── Process Managers
          │
          ├──→ Domain ─── State machines / Invariants / Policies
          │
          └──→ Outbound Ports
                    │
                    ├── Git / Codex / GitHub / Build Adapters
                    ├── Markdown / Event Journal Adapters
                    └── SQLite Projector / Query Adapter
```

依存は外側から内側へ向ける。

```text
Clients / Bootstrap / Adapters → Application → Domain
Runtime → Application / Scheduler → Domain
```

Applicationは具体Adapterを知らない。具体実装の選択とDIは共通bootstrapへ集約する。詳細は[Applicationと外部連携](docs/architecture/application-and-integrations.md)を参照する。

## 6. 正本とRead Model

| 情報 | 正本 |
| --- | --- |
| Vision、原則、仮説、Backlog、Task Contract | Git管理Markdown |
| Decision、Build metadata、Acceptance Result | 正規化Markdown |
| Task Run、Integration Run、外部Operationの履歴 | 追記専用Event Journal |
| commit、branch、worktree | Git |
| PR、check、GitHub Actions結果 | GitHub |
| Build本体、大容量ログ | Artifact Store |
| 一覧、検索、集計、画面状態 | 再構築可能なSQLite Projection |

Markdownは承認された宣言的事実、Event Journalは時間とともに発生する実行事実を保持する。SQLiteだけに制作意図、判断、Acceptanceを保存してはならない。詳細は[データ、正本、整合性](docs/architecture/data-and-consistency.md)を参照する。

## 7. Runtimeと単一Writer

プロジェクトごとに一つの`ProjectCoordinator`がMutation Ownerとなる。

- Event JournalはCoordinator内の単一Appenderが追記する。
- SQLiteはCoordinator内の単一Projector Writerが更新する。
- Markdown、Git、Codex、GitHub、BuildへのMutationはCoordinatorを通す。
- DesktopとCLIは通常Clientとして既存Coordinatorへ接続する。
- Coordinatorが存在しない場合だけ、排他的な`ProjectWriterLease`取得後に起動する。
- 同時起動の敗者はwrite connectionや具体Adapterを開かない。
- Commandは`command_id`で重複排除し、expected versionで競合検出する。
- 強制終了後はJournal、Projection、Git、外部OperationをReconciliationしてからMutationを再開する。

Coordinatorは役割であり、Desktop内包processにもheadless processにもなり得る。IPC、OS別lock、detach方法はADRで決める。詳細は[Runtime、Scheduler、GUI](docs/architecture/runtime-and-ui.md)を参照する。

## 8. 主要な不変条件

### Taskと実行

- Task DAGに存在しない参照、自己依存、循環がない。
- `blocks_start`と`blocks_integration`の意味を開始・統合順序へ反映する。
- Task RunはTask Contract revisionとbase commitへ固定する。
- 差分は`allowed_paths`内かつ`forbidden_paths`外でなければ統合しない。
- 修正、再依頼、自動修正は既存Runを上書きせず、新しいRunとして残す。

### Evidence、Build、Acceptance

- Evidenceは現在のIntegration Candidate commitと一致するものだけ有効である。
- Buildは一意のcommit SHAと含有Task Run集合を持つ。
- Acceptance ResultはPlaytest Session、Build、commit SHAへ固定する。
- commit変更後はCI、AI Review、Build、Acceptanceを再利用しない。
- 同一commitが技術検証済みかつ人間に受け入れられた場合だけ`main`へ進める。
- `UNKNOWN`は`SAFE`ではない。

### TDD

- 振る舞いを変更するTask Runは、production変更前のRed証拠を持つ。
- Red確認後にだけproduction pathを解放する。
- Greenと最終suiteの成功がなければTask Runを`SUCCEEDED`にしない。
- testの削除、skip、assertionの弱体化だけでGreenにしてはならない。

詳細は[ドメイン設計](docs/architecture/domain.md)と[TDDとテスト戦略](docs/architecture/testing.md)を参照する。

## 9. 中核フロー

```text
Intent
  → Task Contract / Dependency DAG
  → ProjectCoordinator / Scheduler
  → TDD: Red → Green → Refactor
  → Local Checks / Scope Check
  → Conflict Analysis / Integration Candidate
  → GitHub Actions / Independent AI Review
  → Build
  → Human Playtest / Acceptance
  → 同一commitをmainへmerge
```

この流れはIntent to Work、Work to Evidence、Evidence to Decisionの三つの変換を実現する。完全な処理順序は[Runtime、Scheduler、GUI](docs/architecture/runtime-and-ui.md)、実装順序は[TDDとテスト戦略](docs/architecture/testing.md)を参照する。

## 10. 未確定事項

未確定事項は本文へ曖昧に埋め込まず、選択肢、決定理由、MVPへの影響、再検討条件をADRへ記録する。現在の候補は[運用、監査、設計判断](docs/architecture/operations.md)に集約する。

本書にはシステム全体へ影響する境界と不変条件だけを置く。特定crateの内部構造、物理ファイル形式、SQL DDL、OS別実装などは詳細文書またはADRへ置き、概要の肥大化を避ける。
