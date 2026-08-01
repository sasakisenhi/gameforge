# ドメイン設計

> [アーキテクチャ概要](../../architecture.md)へ戻る

Bounded Context、Functional Core、Aggregate、不変条件、状態機械を定義する。

## ドメイン分割

### Bounded Context

#### Intent & Planning

遊びの核を、承認可能な計画とTask Contractへ変換する。

- `GameVision`
- `DesignPrinciple`
- `Hypothesis`
- `Milestone`
- `ProductBacklogItem`
- `AcceptanceCriterion`
- `Task`
- `TaskContract`
- `TaskDependency`

主要な規則は、Task DAGの非巡回性、依存先の存在、Task Contractのパス妥当性、Plan Review承認前に実行制約を有効化しないことである。

#### Work Execution

承認済みTaskを、隔離された実行とローカル証拠へ変換する。

- `TaskRun`
- `AgentRun`
- `SchedulerQueueItem`
- `WorktreeRef`
- `BranchRef`
- `LocalCheck`
- `ScopeCheck`

Codex processやOS process handleそのものはドメインオブジェクトではなく、実行Adapterが所有する資源である。Domainはそれらを安定したIDと観測された状態で参照する。

#### Integration & Verification

複数Task Runを一意のcommitへ統合し、技術的証拠を収集する。

- `IntegrationCandidate`
- `IntegrationRun`
- `VerificationRun`
- `CICheck`
- `AIReview`
- `ReviewFinding`
- `ConflictAnalysis`
- `Evidence`

主要な規則は、依存順序を守ること、同一シンボル変更を直列化すること、必須checkとAI Reviewが対象commitに一致すること、不明な競合を安全扱いしないことである。

#### Acceptance & Learning

検証済みcommitを人間のプレイ判断と次の反復へ接続する。

- `Build`
- `PlaytestScenario`
- `PlaytestSession`
- `AcceptanceResult`
- `Observation`
- `ManualAdjustment`

主要な規則は、Acceptance ResultをプレイしたBuildとcommitへ固定すること、commit変更時に受け入れを無効化すること、人間の受け入れなしに`main`へ進めないことである。

#### Human Decision

複数Contextを横断して、人間にしか行えない判断を扱う。

- `DecisionRequest`
- `DecisionOption`
- `Decision`
- `HumanInboxItem`
- 型付き`SubjectRef`

自由形式の汎用オブジェクトグラフにはせず、MVPで認める参照型を列挙する。判断結果による他Aggregateの変更は、Application層のProcess Managerが個別Commandとして調停する。

### Context間の連携

- Context間は安定した型付きIDとDomain Eventで連携する。
- 一つのContextから別Contextの内部状態を直接変更しない。
- 複数Aggregateにまたがる処理はApplication層のProcess Managerが順序付ける。
- 画面表示のための横断JoinはDomain ModelではなくSQLite Projectionで行う。
- Context間で共有する型は、ID、commit SHA、hash、時刻、限定された列挙型などの小さなShared Kernelに留める。

## Domain Modelの実装方針

### Functional Core

状態遷移、可否判定、優先順位、証拠の有効性は、I/Oを行わない純粋関数として実装する。

```rust
fn decide(state: &TaskRun, command: TaskRunCommand)
    -> Result<Vec<TaskRunEvent>, DomainError>;

fn evolve(state: TaskRun, event: &TaskRunEvent)
    -> TaskRun;
```

実際の型名は実装時に調整してよいが、次の性質を維持する。

- 同じ入力から常に同じ結果を返す。
- 現在時刻、乱数、ID採番は引数で受け取る。
- GitやSQLiteを関数内から呼ばない。
- 不正な状態を可能な限り型で表現不能にし、残りを`Result`で拒否する。
- 遷移理由をEventまたは明示的な判定結果として返す。

Event Sourcingを全モデルへ強制しない。実行ライフサイクルはイベントから復元し、人間が編集する文書はMarkdownからロードする。どちらの場合も、遷移規則は同じDomain関数を通す。

### Aggregate候補

MVPでは次を独立した整合性境界の候補とする。

| Aggregate | 一度に守る主な不変条件 |
| --- | --- |
| `Task` | Contract、Task状態、依存定義の局所的妥当性 |
| `TaskRun` | 一回の実行状態、base commit、scope結果、試行履歴 |
| `IntegrationCandidate` | 含有Task Run、統合順序、対象commit、品質ゲート |
| `Build` | artifact、生成元commit、含有Task集合 |
| `PlaytestSession` | プレイ対象Buildと一つのAcceptance Result |
| `DecisionRequest` | 選択肢、回答、対象、未解決・解決状態 |

DAG全体の循環検出や「受け入れによる複数Taskの更新」は単一Aggregateのトランザクションに押し込まず、Domain ServiceとProcess Managerで扱う。

### 主要な不変条件

#### Taskと依存関係

- `READY`になるTaskは、有効なTask Contractを持つ。
- Task依存グラフには、存在しない参照、自己依存、循環がない。
- `blocks_start`の依存先が受け入れ済みでなければTask Runを開始しない。
- `blocks_integration`の依存順序に反して統合しない。
- Contract変更により前提が変わったTask Runは`STALE`として再評価する。

#### Task Runとスコープ

- 一つのTask Runは一つのTask Contract revisionとbase commitへ固定する。
- 実行枠未割当てのTask Runを`AGENT_RUNNING`にしない。
- 差分の実パスが`allowed_paths`内かつ`forbidden_paths`外でなければ統合しない。
- 振る舞いを変更するTask Runは、production code変更より前の`RED`証拠と、変更後の`GREEN`証拠を持たなければ成功・統合できない。
- テストの削除、skip、assertionの弱体化だけで`GREEN`にしてはならない。
- 修正、再依頼、自動修正は既存Runを上書きせず、新しいRunとして記録する。

#### Integration、Build、Acceptance

- Integration CandidateのEvidenceは、現在のintegration commitと一致するものだけ有効である。
- Buildは一意のcommit SHAと含有Task Run集合を持つ。
- Acceptance ResultはPlaytest Session、Build、commit SHAへ固定する。
- 受け入れ後にcommitが変われば、CI、AI Review、Build、Acceptanceを再利用しない。
- `main`へマージできるのは、同一commitが技術検証済みかつ人間に受け入れられたPRだけである。
- merge直前にPR head、required checks、acceptance対象commitを再照合する。

#### 解析と判断

- `UNKNOWN`は`SAFE`ではない。
- 高リスク競合、共有契約変更、設計判断はHuman Inboxを経由する。
- 進捗表示は状態から導出するProjectionであり、直接編集できない。

### 状態機械

Task、Task Run、Integration Candidate、Build、Acceptanceは別々の状態機械として実装する。巨大な共通`Status` enumは作らない。

`STALE`、`SCOPE_VIOLATION`、`CONFLICT_RISK`のような状態は実行段階と直交するため、主状態とは別のHealth/Flagとして表現する。たとえば`AGENT_RUNNING + CONFLICT_RISK`を保持できるようにする。

遷移APIは「次状態を直接setする」のではなく、意図を表すCommandを受ける。

```text
悪い例: task_run.status = SUCCEEDED
良い例: CompleteLocalChecks { evidence_ids }
        → 前提条件を検証
        → LocalChecksCompleted / TaskRunSucceeded
```

