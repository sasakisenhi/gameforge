# AI並列ゲーム開発コントロールプレーン

## MVP仕様 v0.1

## 1. プロダクトの目的

本システムは、ゲーム制作者が定義した「遊びの核」と「ゲームとしての理想像」を、AIによる設計・実装・検証と、人間による方針判断・プレイ受け入れへ接続する開発コントロールプレーンである。

目的は、コードエディタやAIチャットを再実装することではない。

> ゲームの意図を、検証済みのプレイ可能な成果へ、再現可能な手順で変換する。

MVPでは、この変換を人間が理解・承認・操作するためのデスクトップGUIを主要成果物とする。GUIはIntent to Work、Work to Evidence、Evidence to Decisionを一つの連続した体験として提供する。

人間は、ゲームの価値、方向性、トレードオフ、プレイ体験を判断する。AIは、委任された範囲内で設計・実装・テスト・修正を実行する。CIとAIレビューは技術的品質を検証する。

---

# 2. Zedと本システムの責務

## Zedの責務

Zedは、コードの製造・調査・修正を行う開発ワークベンチである。

* ソースコードの閲覧・編集
* diffの確認
* ターミナル操作
* デバッグ
* テストの詳細確認
* worktreeの閲覧
* 人間による手動修正
* Codexとの補助的な対話

Zedが扱う中心単位は、ファイル、コード、diff、ターミナル、worktreeである。

## 本システムの責務

本システムは、開発ライフサイクル全体を管理する制作統制層である。

* 遊びの核とゲームデザイン原則の管理
* 仮説、受け入れ基準、バックログの管理
* AIによる仕様・設計・タスク分解
* 複数Codexへのタスク割当て
* Task Contractによるスコープ制御
* worktree間の競合解析
* Codex、CI、AIレビューの状態集約
* 人間の意思決定事項の集約
* プレイ可能なビルドの管理
* プレイ受け入れ結果の記録
* 次の反復へのフィードバック

本システムが扱う中心単位は、意図、仮説、判断、タスク、証拠、ビルド、受け入れ結果である。

---

# 3. MVPの中核フロー

```text
遊びの核・理想像を定義する
        ↓
AIが仕様候補・設計・受け入れ基準・タスクへ分解する
        ↓
人間が重要な方針だけ判断する
        ↓
AIが個別worktreeで並列に設計・実装・テストする
        ↓
依存関係と競合リスクに基づきIntegration Candidateへ統合する
        ↓
GitHub Actionsと独立AIが機械的・技術的品質を検証する
        ↓
検証済みcommitからプレイ可能なBuildを生成する
        ↓
人間が実際に遊んで受け入れ判断する
        ↓
┌────────────┬────────────┬────────────┬────────────┐
│受け入れてmainへ│AIへ修正依頼 │新規タスク化 │手動で微調整│
│マージする      │              │              │              │
└──────┬─────┴──────┬─────┴──────┬─────┴──────┬─────┘
       └─────────────次の反復へ────────────────────┘
```

MVPの価値は、この一周を途中で情報が失われることなく実行できる点にある。

## 受け入れとmainマージの原則

Task Runの成果は、技術検証を通過しただけでは`main`へマージしない。

複数のTask Runを、依存関係と競合解析が示す順序で一時的な`Integration Candidate`ブランチへ統合する。このブランチからGitHub上にdraft PRを作り、GitHub Actions、AIレビュー、Build生成、プレイ受け入れを行う。

人間が受け入れた場合に限り、対象のIntegration Candidateを`main`へマージする。

次の不変条件を守る。

* Buildは一意のcommit SHAと、そのcommitへ含まれるTask集合を持つ
* Acceptance Resultは、プレイしたBuildとcommit SHAへ紐付く
* 受け入れ後にcommitが変わった場合、受け入れ結果は無効となる
* `main`が進んだためrebaseが必要になった場合、CI、Build生成、プレイ受け入れを再実行する
* `main`へ直接pushせず、受け入れ済みIntegration CandidateのPRを通してマージする
* 修正依頼、手動調整、競合解消によって差分が変わった場合は新しいTask RunまたはIntegration Runとして履歴を残す

---

# 4. 正本となるプロジェクト文書

遊びの核、ゲームデザイン原則、仮説、受け入れ基準、実装タスクは、ゲームリポジトリ内のMarkdownファイルとして管理する。

これにより、次を実現する。

* Codexが直接参照できる
* Zedから編集できる
* Gitで履歴を管理できる
* PRによるレビューができる
* ゲームコードと制作意図のバージョンを一致させられる
* 本システムがなくても内容を閲覧できる

## 推奨ディレクトリ

```text
game-project/
├─ AGENTS.md
├─ .game-dev/
│  ├─ vision.md
│  ├─ design-principles.md
│  ├─ hypotheses/
│  ├─ milestones/
│  ├─ backlog/
│  ├─ acceptance/
│  ├─ decisions/
│  ├─ tasks/
│  ├─ runs/
│  ├─ builds/
│  └─ events/
└─ crates/
```

## 正本と派生データ

Git管理されたMarkdownを、制作意図と人間が承認した事実の正本とする。

* `vision`、設計原則、仮説、バックログ、Task ContractはMarkdownを正本とする
* Decision、Build、Acceptance Resultも、正規化したMarkdownとして記録する
* Task Runの状態遷移と検証結果は、追記専用のイベント記録として保存する
* 大容量の生ログやBuild本体はGitへ格納せず、URI、ハッシュ、生成元commitだけを記録する
* SQLiteはGUIの検索、集約、状態表示を高速化するread modelとして使用する
* SQLiteはMarkdownとイベント記録から再構築可能でなければならない
* SQLiteだけに、制作意図、判断、受け入れ結果を保存してはならない

文書形式とイベント形式には`schema_version`を持たせ、読み込み時に検証する。GUIと外部エディタによる同時変更を検出した場合は、内容を上書きせずHuman Inboxへ送る。

## Markdownの形式

文書にはYAML front matterを付け、機械可読にする。

```md
---
id: TASK-002
title: 砂の落下規則
status: ready
parent_backlog: PB-001
hypotheses:
  - HYP-001
acceptance_criteria:
  - AC-001
dependencies:
  - task_id: TASK-001
    kind: blocks_start
    reason: Grid APIの確定が必要
allowed_paths:
  - crates/game_logic/src/sand/**
  - crates/game_logic/tests/sand/**
forbidden_paths:
  - crates/game_runtime/**
risk: low
---

# 目的

空きセルが下にある場合、砂を下方向へ移動させる。

# 非目標

- 水との相互作用
- 砂の圧縮
- 描画演出
```

## `AGENTS.md`の役割

`AGENTS.md`には長大な企画情報を記載せず、Codexが読むべき文書と作業規則を記載する。

```md
# Project guidance

作業前に以下を参照すること。

- `.game-dev/vision.md`
- `.game-dev/design-principles.md`
- 対象タスクが参照する仮説
- 対象タスクの受け入れ基準
- `.game-dev/tasks/<task-id>.md`

対象タスクのallowed_paths外を変更してはならない。
変更が必要な場合は意思決定依頼を作成すること。
```

---

# 5. 中心となるデータモデル

## 制作意図

* `GameVision`
* `DesignPrinciple`
* `Hypothesis`
* `Milestone`
* `ProductBacklogItem`
* `AcceptanceCriterion`

## 意思決定

* `DecisionRequest`
* `DecisionOption`
* `Decision`
* `HumanInboxItem`

## AI実行

* `Task`
* `TaskDependency`
* `TaskContract`
* `TaskRun`
* `AgentRun`
* `Worktree`
* `Branch`
* `PullRequest`
* `SchedulerQueueItem`

## 検証

* `VerificationRun`
* `CICheck`
* `AIReview`
* `ReviewFinding`
* `ConflictAnalysis`
* `IntegrationCandidate`
* `IntegrationRun`
* `Evidence`

## プレイ受け入れ

* `Build`
* `PlaytestScenario`
* `PlaytestSession`
* `AcceptanceResult`
* `Observation`
* `ManualAdjustment`

## 中心的な関連

中心モデルは単純な木ではなく、IDで相互参照される証拠グラフとして扱う。

* 一つのHypothesisは複数のProduct Backlog Itemから検証される
* 一つのAcceptance Criterionは複数のTaskから満たされる場合がある
* 一つのTaskは複数のAcceptance Criterionへ寄与できる
* 一つのDecisionは複数のTask、Build、Hypothesisへ影響できる
* 一つのBuildは複数のTask Runを含む
* Acceptance ResultはBuild、commit、Hypothesisへ証拠を返す

```text
GameVision ── Hypothesis ── ProductBacklogItem
                    │              │
                    └── AcceptanceCriterion
                               │   ▲
                               ▼   │
Task ── TaskRun ── Evidence ── Build ── AcceptanceResult
 │         │          │          │              │
 ├─ dependencies      ├─ CI      ├─ commit      └─ Hypothesis
 ├─ Decision          ├─ Review  └─ included_tasks
 └─ allowed_paths     └─ ConflictAnalysis
```

各関連には安定したIDを使用し、削除ではなく状態変更によって履歴を残す。UIは用途に応じて、このグラフをTask、Build、Hypothesisの各視点から投影する。

## MVPで固定する関連

任意のノードを自由に結べる汎用グラフとして実装せず、MVPでは次の関連を型として定義する。

* TaskとAcceptance Criterion：多対多
* TaskとTask Run：一対多
* Task同士：依存種別を持つ多対多のDAG
* Integration CandidateとTask Run：統合順序を持つ多対多
* BuildとIntegration Candidate：多対一。ただしBuildは一意のcommit SHAを持つ
* Playtest SessionとBuild：多対一
* Acceptance ResultとPlaytest Session：一対一
* Decisionと対象要素：型付き`SubjectRef`による多対多
* EvidenceとCI、Review、Conflict Analysis、Build：型付き参照

次の点はMilestone 0で実例を作ってから確定する。

* Manual Adjustmentを独立したTask Run種別にするか、Task Runへ付随するイベントにするか
* Acceptance ResultをPlaytest Scenarioごとに分割するか、Session全体の判定を中心にするか
* Hypothesis評価をAcceptance Resultから直接更新するか、複数BuildのObservationを集約して別途判断するか

この未確定部分をSQLite固有のテーブル設計へ先に固定せず、Markdown例とdomain型で検証する。

---

# 6. TaskとTask Runの区別

## Task

何を実現するかを表す論理的な作業である。

* 目的
* 非目標
* 受け入れ基準
* 依存関係
* 許可されたファイル範囲
* 必須検証
* リスク

## Task Run

Taskを実際にAIへ依頼した一回の実行である。

```text
Task
└─ Task Run #1
   ├─ Codex thread
   ├─ worktree
   ├─ branch
   ├─ base commit
   ├─ local verification
   └─ scope check

Integration Candidate
├─ included Task Runs
├─ integration commit
├─ draft PR
├─ GitHub Actions
├─ AI review
└─ Build / Acceptance
```

同じTaskを修正のために再実行する場合は、別のTask Runとして記録する。

これにより、失敗した実行、再依頼、別案の試作を区別できる。

## Taskの依存関係

Taskの依存関係は有向非巡回グラフとして管理する。Task Contractでは単なるID一覧ではなく、依存理由とブロック対象を記録する。

```yaml
dependencies:
  - task_id: TASK-001
    kind: blocks_start
    reason: Grid APIの確定が必要
  - task_id: TASK-004
    kind: blocks_integration
    reason: 共通設定ファイルを先に統合する
```

MVPでは次の二種類を扱う。

* `blocks_start`：依存先が受け入れられ`main`へマージされるまで、Task Runを開始しない
* `blocks_integration`：Task Runは並列に開始できるが、依存先より先にIntegration Candidateへ統合しない

Taskを`READY`へ変更する前に、存在しないTaskへの参照、自己依存、循環依存を検出する。依存先のTask Contractや公開APIが変更された場合は、依存するTask Runを`STALE`として再評価する。

Development Boardでは、ブロックしているTask、ブロックされているTask、クリティカルパス、並列実行可能なTask集合を表示する。AIが提案した依存関係は、人間がPlan Reviewで承認してから実行制約として有効にする。

---

# 7. Codex連携

MVPでは、本システムがCodex App Serverをローカル子プロセスとして起動し、Task Run単位でCodexを制御する。

Zedは、生成されたworktreeを人間が確認・修正するための作業場として使用する。

## MVPで利用する機能

* App Serverの初期化
* thread開始・再開
* turn開始
* エージェントメッセージ取得
* コマンド実行状態取得
* ファイル変更状態取得
* turn完了・失敗取得
* 承認要求の受信
* ユーザー入力要求の受信
* turn中断

## 並列実行モデル

並列数をドメインモデルへ固定値として埋め込まない。アプリケーションは任意個のTask Runをキューへ登録でき、Schedulerが実行容量、依存関係、競合リスクに基づいて開始可否を決定する。

```text
Ready Task Runs
      ↓
Dependency / Conflict / Resource Check
      ↓
Scheduler ── max_concurrent_task_runs
      ↓
N × Codex Process + Worktree
```

MVPの既定値は`max_concurrent_task_runs = 3`とする。ただし設定で増減でき、Queue、Task Run、Agent Run、WorktreeのデータモデルとUIは任意個を扱える設計にする。

Schedulerは少なくとも次を考慮する。

* `blocks_start`依存関係
* `allowed_paths`の重複
* 同時変更する重要ファイルとシンボル
* CPU、メモリ、実行プロセス数
* 人間の承認待ち
* Taskごとの優先度とリスク

実行枠を超えたTask Runは失敗ではなく`QUEUED`として待機する。並列数を増やしても、Task Contract、状態遷移、証拠記録、承認の意味が変わらないことを設計条件とする。

## Task Run開始時の処理

```text
Task Contract検証
        ↓
依存DAG・実行可能条件確認
        ↓
競合リスクの事前解析
        ↓
Schedulerが実行枠を割り当て
        ↓
branch・worktree作成
        ↓
Codex thread開始
        ↓
Task Contractと関連文書を投入
        ↓
Agent Runningへ遷移
```

## Zedとの連携

各Task Runには次の操作を用意する。

* 対象worktreeをZedで開く
* 変更ファイルをZedで開く
* AIレビュー対象行をZedで開く
* CI失敗に関連するコードをZedで開く

本システム内にコードエディタや汎用ターミナルは作らない。

## Task Contractのスコープ強制

`allowed_paths`と`forbidden_paths`はCodexへの説明だけではなく、機械的な品質ゲートとして扱う。

1. Task Run開始前に、パスをリポジトリルート基準へ正規化し、重複、親ディレクトリ参照、無効なglobを検証する
2. CodexへTask Contractを渡し、利用可能な実行sandboxがある場合は同じ範囲へ設定する
3. turn完了時と手動修正後に、base commitとの差分から追加、変更、削除、renameを取得する
4. 実パスへ解決した変更が`allowed_paths`内かつ`forbidden_paths`外であることを検査する
5. 違反があれば`SCOPE_VIOLATION`としてIntegration Candidateへの統合を停止する

例外を暗黙に許可しない。許可範囲の変更が必要な場合はDecision Requestを作成し、人間がTask Contractを更新した後、新しいTask Runとして再実行する。

---

# 8. タスク状態機械

進捗率をAIに推測させず、観測可能なイベントから状態を導出する。Task、Task Run、統合、Build、プレイ受け入れは独立したライフサイクルとして管理し、一つの巨大な状態列挙へ混在させない。

## Task lifecycle

```text
DRAFT → READY → ACTIVE → ACCEPTED
   └───────→ CANCELLED

READY → WAITING_DEPENDENCY
WAITING_DEPENDENCY → READY
```

`ACCEPTED`は、Taskを含むBuildが人間に受け入れられ、対応commitが`main`へマージされたときに成立する。

## Task Run lifecycle

```text
QUEUED → PREPARING → AGENT_RUNNING → LOCAL_CHECKING → SUCCEEDED
   │          │             │               └→ FAILED
   │          │             ├→ INPUT_REQUIRED
   │          │             └→ DECISION_REQUIRED
   └──────────┴──────────────────────────────→ CANCELLED
```

追加フラグとして`STALE`、`SCOPE_VIOLATION`、`CONFLICT_RISK`を持てる。これらは実行段階と直交するため、主状態へ混在させない。

## Integration Candidate lifecycle

```text
DRAFT → COMPOSING → LOCAL_CHECKING → PR_OPEN
  → CI_RUNNING → AI_REVIEWING → TECHNICALLY_VERIFIED
  → BUILDING → PLAYTEST_REQUIRED
  → ACCEPTED → MERGING → MERGED

COMPOSING / CHECKING / CI / REVIEW / BUILD
  └→ FAILED → REVISING

PLAYTEST_REQUIRED
  ├→ DEFECT → REVISING
  ├→ ENHANCEMENT → ACCEPTED または新規Task
  └→ MANUAL_TUNING → REVISING
```

`ACCEPTED`後にcommit SHAが変化した場合は`STALE`へ遷移し、`TECHNICALLY_VERIFIED`以前へ戻す。

## Build lifecycle

```text
REQUESTED → BUILDING → READY
                 └→ FAILED

READY → SUPERSEDED
```

## Acceptance lifecycle

```text
PENDING → PLAYTESTING
          ├→ ACCEPTED
          ├→ DEFECT
          ├→ ENHANCEMENT
          └→ MANUAL_TUNING
```

Development Boardに表示する総合状態は、これらの状態を優先順位付きで投影した値であり、正本として直接編集しない。

---

# 9. Development Board

複数Codexの会話一覧ではなく、Task Runと品質ゲートの状態を表示する。

| Task  | Agent   | 状態            | Local | CI      | AI Review | Conflict | Build |
| ----- | ------- | ------------- | ----- | ------- | --------- | -------- | ----- |
| Grid  | Codex-1 | Accepted      | Pass  | Pass    | Pass      | Safe     | #12   |
| Sand  | Codex-2 | CI Failed     | Pass  | Unit NG | ―         | Low      | ―     |
| Water | Codex-3 | Agent Running | ―     | ―       | ―         | High     | ―     |
| Input | ―       | Queued        | ―     | ―       | ―         | Safe     | ―     |

Task Run詳細には次を表示する。

* 依頼内容
* 受け入れ基準
* 実装タスク
* 依存Taskと依存種別
* ブロック理由と統合順序
* Codex thread
* worktree
* branch
* base commit
* 許可パス
* 実際に編集されたファイル
* 現在の実行段階
* CI結果
* AIレビュー
* 競合解析
* 意思決定依頼
* 残余リスク

Board上部には、実行中／実行可能／依存待ち／人間待ちの件数と、現在の並列実行上限を表示する。上限変更はScheduler設定へ反映するが、実行中Task Runを暗黙に中断しない。

---

# 10. CI統合

リモートで再現可能な技術検証結果はGitHub Actionsを正本とし、本システムはcheck runをTask Run、Integration Candidate、commit、受け入れ基準へ関連付ける。

## ローカル検査とGitHub Actionsの役割

ローカル検査は、Codexが変更を完了した直後に高速なフィードバックを与える。GitHub Actionsは、クリーンな環境で同じ検査を再実行し、Integration CandidateをBuild・プレイ受け入れへ進めてよいかを判定する品質ゲートである。

```text
Task Run
  → Local Checks
  → Integration Candidate / draft PR
  → GitHub Actions
  → AI Review
  → Technically Verified
  → Build
```

ローカル検査の成功だけでBuild生成や`main`マージを許可しない。必須GitHub Actionsが失敗、未実行、cancelled、または対象commitと不一致の場合、Integration Candidateは技術検証済みにならない。

## GitHub Actionsとの連携

* Integration Candidateのpushとdraft PRを契機にworkflowを実行する
* repository、workflow、run、job、check、commit SHAを保存する
* required checkの集合はリポジトリ設定とTask Contractから決定する
* GitHubのcheck名を内部の検査種別とAcceptance Criterionへ対応付ける
* rerunは同じVerification Runの上書きではなく、新しい試行として記録する
* main branch protectionを前提とし、受け入れ済みPR以外からの直接マージを行わない
* ネットワーク障害やGitHub認証失敗はテスト失敗と区別して表示する
* GitHub Actionsが利用できない間もローカル作業は継続できるが、Build受け入れとmainマージは保留する

## MVPで扱う検査

* ビルド
* format
* lint
* 単体テスト
* 結合テスト
* アーキテクチャテスト
* スコープ違反
* 依存関係監査
* ライセンス監査

## 表示例

```text
TASK-002 砂の落下規則

Build                 PASS
Format                PASS
Clippy                PASS
Unit Test             FAIL
Architecture          PASS
Scope Policy          PASS

失敗：
sand_stops_at_world_boundary

関連する受け入れ基準：
AC-002 ワールド境界を越えない
```

CIログそのものはGitHubまたはZedから確認する。本システムはログビューアーを再実装せず、失敗したjob、要約、関連Task、関連Acceptance Criterion、再実行状態、GitHubへのリンクを表示する。

---

# 11. AIレビュー

実装を担当したCodexとは別のCodexがレビューする。

## MVPのレビュー観点

* 受け入れ基準との整合性
* 正当性
* 境界条件
* 回帰リスク
* テスト不足
* アーキテクチャ違反
* スコープ逸脱
* 過剰な複雑化
* 残余リスク

## レビュー結果の形式

```json
{
  "verdict": "needs_changes",
  "findings": [
    {
      "severity": "high",
      "category": "correctness",
      "symbol": "Grid::set",
      "file": "crates/game_logic/src/grid.rs",
      "claim": "ワールド下端で範囲外アクセスが起きる",
      "evidence": "境界判定より先にy - 1を計算している",
      "suggested_test": "sand_stops_at_world_bottom",
      "status": "open"
    }
  ],
  "residual_risks": [
    "大量セルでの性能は未検証"
  ]
}
```

レビュー結果は、重要度、カテゴリ、Task、状態別に一覧表示する。

---

# 12. Structural Conflict Analyzer

MVPには、Rustを対象とした構造・シンボル単位の競合解析を含める。

「意味的競合を完全に証明する」のではなく、複数worktreeの変更が統合時に干渉する可能性を早期検出する。

## 解析レベル

### レベル0：ファイル競合

* 同一ファイルの変更
* `allowed_paths`の重複
* 重要設定ファイルの重複

### レベル1：AST・シンボル競合

* 同一関数の変更
* 同一構造体の変更
* 同一列挙型の変更
* 同一トレイトの変更
* 同一`impl`の変更
* 関数シグネチャの変更
* visibilityの変更
* フィールド・variantの変更
* 要素の追加・削除

### レベル2：参照関係による影響分析

* 定義側と呼び出し側の変更
* 型定義と`impl`の変更
* enum variant追加と`match`側の変更
* trait変更と実装側の変更
* 公開API変更の影響

MVP初期はAST解析を中心とし、参照解決は段階的にrust-analyzer連携を追加する。

MVPではレベル0を必須とし、レベル1はRustの通常の関数、構造体、列挙型、trait、`impl`に対象を限定して実装する。macro展開後の意味、生成コード、完全な名前解決は`UNKNOWN`として扱う。レベル2の完全な参照解決はMVP後へ段階化する。

## 判定結果

* `SAFE`
* `POTENTIAL_CONFLICT`
* `CONFLICT`
* `UNKNOWN`

`UNKNOWN`を安全として扱ってはならない。

## 高リスク例

* 同じ関数を二つのTask Runが変更
* 一方が関数を削除し、他方が変更
* 一方がtraitを変更し、他方がそのtraitを実装
* 一方がシグネチャを変更し、他方が呼び出し側を変更
* 一方がenum variantを追加し、他方が網羅的な`match`を変更

## マージ順序の規則

1. 明示的な依存タスクを先にマージする
2. 共有契約の変更を先にマージする
3. 定義側を参照側より先にマージする
4. シグネチャ変更を関数本体だけの変更より先にする
5. 同一シンボル変更は直列化する
6. 後続Task Runを最新mainへrebaseして再検証する
7. 判断不能ならHuman Inboxへ送る

## 自動rebase

SchedulerはTask RunまたはIntegration Candidateのbaseが古くなったことを検出できる。次の条件をすべて満たす場合に限り、自動rebaseを実行する。

* 対象に未記録の人間による変更がない
* 進行中のCodex turnがない
* Gitが競合なしでrebaseを完了できる
* Task Contractの許可範囲を逸脱しない

競合が発生した場合は自動解消せず、rebaseを中止して元の状態を保ち、`CONFLICT_RISK`としてHuman Inboxへ送る。rebaseによってcommit SHAが変わった場合、それ以前のCI、AIレビュー、Build、Acceptance Resultを再利用しない。

## 自動修正

Local Check、GitHub Actions、AIレビューで修正可能な失敗が見つかった場合、ポリシーと再試行上限の範囲内でCodexへ自動修正を依頼できる。

* 自動修正は元Taskを上書きせず、新しいTask Runとして記録する
* 失敗した検査、関連Acceptance Criterion、差分、残余リスクを入力へ含める
* `allowed_paths`と`forbidden_paths`を維持する
* 同一原因での再試行回数に上限を設ける
* 設計判断、共有契約変更、依存関係変更が必要な場合はHuman Inboxへ送る
* 自動修正後はLocal Check以降のすべての品質ゲートを再実行する

自動rebaseと自動修正は、`main`への自動マージ、人間の代わりのプレイ受け入れ、競合の推測による解消を意味しない。

## 表示例

```text
競合リスク：高

TASK-002 砂の落下
TASK-003 水の流動

競合シンボル：
Grid::set

TASK-002：
戻り値を () から Result<(), GridError> へ変更

TASK-003：
境界外座標を無視する処理を追加

推奨順序：
1. TASK-002をマージ
2. TASK-003を最新mainへrebase
3. Result型へ対応
4. CIを再実行
```

---

# 13. Human Inbox

Human Inboxは、人間にしか処理できない判断を一画面へ集約する。

## MVPで集約する項目

* ゲームデザイン上の方針判断
* 設計トレードオフ
* 要求の曖昧性
* スコープ変更
* 共有契約変更
* 高リスク競合
* マージ順序の判断
* CI例外
* AIレビュー上の重大な指摘
* AI同士の判断対立
* プレイ受け入れ
* AIからの承認・追加情報要求

## Human Inbox項目

```text
[高] Material traitを変更するか

理由：
火と水の両方で温度を扱う必要が生じた。

選択肢：
A. Material traitへtemperatureを追加
B. Temperatureコンポーネントへ分離
C. 今回は温度表現を見送る

AI推奨：
B

影響：
TASK-012、TASK-013、TASK-016をブロック中
```

## 操作

* 推奨案を採用
* 別案を採用
* 追加調査を依頼
* タスクを分割
* スコープを変更
* 並列実行を許可
* 人間による手動対応へ切り替え
* 判断を保留

判断結果は、Task Contract、Decision文書、Codexへの追加指示へ反映する。

---

# 14. プロダクトバックログの進捗

単純なタスク件数ベースの進捗率を主指標にしない。

バックログ項目ごとに、価値到達状態を表示する。

```text
PROPOSED
↓
APPROVED
↓
DECOMPOSED
↓
IMPLEMENTING
↓
TECHNICALLY_VERIFIED
↓
PLAYABLE
↓
ACCEPTED
↓
HYPOTHESIS_EVALUATED
```

表示例：

| Backlog | 実装  | 技術検証 | Playable | 人間受入     | 仮説評価      |
| ------- | --- | ---- | -------- | -------- | --------- |
| 砂の基本挙動  | 完了  | 完了   | Yes      | Accepted | Supported |
| 水の基本挙動  | 実装中 | 一部失敗 | No       | ―        | ―         |
| 火との反応   | 未着手 | ―    | No       | ―        | ―         |

タスク完了率は補助情報として扱う。

---

# 15. Build & Acceptance

技術検証を通過した成果物は、Buildとして登録する。

## Buildに含める情報

* Build ID
* commit
* Integration Candidateとbranch
* 含まれるTask
* 含まれるTask Run
* 対象マイルストーン
* 対応プラットフォーム
* artifact URIとハッシュ
* CI結果
* AIレビュー結果
* 残余リスク
* プレイテスト状態

## プレイ開始画面

```text
First Playable #1
Build 0042

今回追加されたもの：
- 砂の配置
- 砂の落下
- ワールドリセット

確認してほしいこと：
1. 砂を直感的に配置できるか
2. 落下挙動が理解可能か
3. 追加で配置して結果を見たくなるか

[ゲームを起動]
```

## プレイ後の分岐

### 受け入れる

* Acceptance ResultをBuildとcommit SHAへ記録
* Integration Candidateのdraft PRをreadyへ変更
* `main`との差分と必須checkが変わっていないことを再確認
* 対象PRを`main`へマージ
* マージ成功後にTaskを`ACCEPTED`へ変更
* バックログと仮説の状態を更新
* `blocks_start`で待機していた後続Taskを再評価

受け入れ後に`main`が進み、そのままマージできない場合は受け入れ済みと表示したまま強制マージしない。Integration Candidateを更新し、CI、Build生成、プレイ受け入れをやり直す。

### AIへ修正依頼

* 元Taskを再開
* 新しいTask Runを作成
* 保存した再現情報をCodexへ渡す
* Integration Candidateと旧Buildを`SUPERSEDED`として残す

### 新規タスク化

* 仕様どおりだが改善したい事項
* 新しいProduct Backlog ItemまたはTaskとして登録

### 手動で微調整

* Zedでworktreeを開く
* 人が数値や挙動を調整
* 変更理由を記録
* 必要なら受け入れ基準や制作原則を更新
* 変更後のcommitに対して品質ゲートとプレイ受け入れを再実行

---

# 16. MVPの画面構成

GUIは、三つの変換を人間が理解・承認・追跡するための主インターフェースである。

```text
Intent to Work        Project Intent + Plan Review
Work to Evidence      Development Board + Task Run Detail
Evidence to Decision  Human Inbox + Build & Acceptance
```

GUIの目的は、Codexのチャット、GitHubのログ、Zedのコード編集機能を一画面へ複製することではない。人間が現在の意図、委任された作業、得られた証拠、必要な判断を往復せず把握できることを目的とする。

すべての主要操作には、対象、影響、根拠、次に起きる状態遷移を表示する。詳細なコード、diff、ログはZedまたはGitHubへdeep linkする。

## 1. Project Intent

* 遊びの核
* ゲームデザイン原則
* 仮説
* First Playable
* プロダクトバックログ
* 受け入れ基準

リポジトリ内Markdownを読み書きする。

## 2. Plan Review

* AIが提案した仕様
* 設計候補
* Decision Card
* タスク分解
* 依存関係
* 依存種別と依存理由
* クリティカルパス
* 並列実行計画
* 人間による承認

## 3. Development Board

* Task Run
* 実行Queueと並列実行上限
* Codex状態
* worktree
* branch
* 変更ファイル
* CI
* AIレビュー
* 競合リスク
* マージ順序
* Integration Candidate

## 4. Human Inbox

* 未処理の意思決定依頼
* 重大度
* 推奨案
* 影響範囲
* ブロック中タスク
* 回答操作

## 5. Build & Acceptance

* 利用可能なBuild
* 含まれるTask
* 技術検証結果
* 受け入れシナリオ
* ゲーム起動
* 受け入れ・修正・新規タスク・手動調整
* 対象commitとmainマージ可否

MVPでは五つの独立した大型アプリを作らず、単一ウィンドウ内の画面またはタブとして実装する。最初の縦切りでは、Development Board、Human Inbox、Build & Acceptanceを優先し、Project IntentとPlan ReviewはMarkdown編集と承認に必要な最小UIに留める。

---

# 17. 技術構成

## 主言語

Rust

## デスクトップUI

Dioxus Desktopを第一候補とする。

GUIを三つの変換を扱う主インターフェースとする。GUIはapplication層のUse Caseを呼び出し、Task Run、検証、判断、Buildのread modelを購読する。Git、Codex、GitHub ActionsをGUIコンポーネントから直接操作しない。

## 非同期処理

Tokio

## データ保存

SQLiteを検索・集約用read modelとして使用する。制作意図と承認済みの事実はMarkdown、実行履歴はイベント記録を正本とし、SQLiteは再構築可能にする。

## Codex連携

Codex App Serverをstdio上のJSON-RPCで制御する。

Task Runごとにthread、turn、process、worktreeを識別し、複数のApp Serverまたは実行セッションをSchedulerから管理する。プロトコルのバージョン差異、プロセス終了、再起動後の再接続、承認待ちをadapter層で吸収する。

## Git操作

初期はGit CLIをラップする。必要に応じてGitライブラリへ置き換える。

branch、worktree、rebase、Integration Candidate、commit SHA、diffの取得をgit-adapterへ集約する。すべての破壊的操作は対象を明示し、実行前後のHEADとworktree状態をイベントとして記録する。

## Rust構文解析

* `syn`によるAST解析
* 後続段階でrust-analyzer連携

## ゲーム

Rust・Bevy

## ゲーム内受け入れHUD

MVPでは必須としない。デスクトップGUIからゲームを起動し、終了後に受け入れ結果を入力する。ゲーム内から観察やスクリーンショットを送る必要性が確認された後、`bevy_egui`によるHUDを追加する。

## CI

GitHub Actions

GitHub Actionsはクリーン環境での技術検証を担当する。本システムはworkflow自体を置き換えず、実行要求、状態取得、TaskとAcceptance Criterionへの関連付け、失敗要約、GitHubへのリンクを担当する。

## エディタ

Zed

Zedはコード、diff、ログ、テストを詳細に調査し、人間が手動修正するための開発ワークベンチである。本システムは次の連携を提供する。

* Task RunのworktreeをZedで開く
* 変更ファイルと対象行をZedで開く
* Review Finding、CI失敗、競合シンボルから関連箇所を開く
* 手動修正前に対象Task Runと許可パスを表示する
* Zedで行われた変更をdiffとして再検出し、Task RunまたはManual Adjustmentへ関連付ける
* 手動修正後にLocal Check、CI、Build、Acceptanceの再実行を要求する

Zedで変更したこと自体をスコープ違反とはしない。ただし、変更者にかかわらず`allowed_paths`を検査し、正本となるTask Contractと証拠の連鎖を維持する。本システムはZedの編集内容を無断で上書きしない。

## CLI

同じapplication/domain層を使用する、決定論的な補助インターフェースとして提供する。

CLIは次を担当する。

* 初期化、文書検証、SQLite read modelの再構築
* Task DAG、Task Contract、スコープの検査
* Task Runの開始、再開、中断、状態取得
* Local Check、競合解析、Build生成の単独実行
* GUIを起動できない環境での診断と復旧
* CIやスクリプトから利用するmachine-readableなJSON出力

GUIは日常の理解・承認・判断を担当し、CLIは自動化・診断・復旧を担当する。両者は別々のビジネスロジックを持たず、同じUse Caseと状態遷移を呼び出す。GUIが内部でCLIの表示文字列を解析する構成にはしない。

## 責務と連携の境界

| 要素 | 正本となる情報・操作 | 本システムとの連携 |
| --- | --- | --- |
| GUI | 意図、証拠、判断の可視化と人間の承認 | application層のUse Caseとread model |
| CLI | 自動化、検査、診断、復旧 | GUIと同じUse Caseを人間向け／JSON形式で公開 |
| Zed | コード、diff、詳細ログ、手動修正 | worktree・ファイル・行へのdeep linkと変更再検出 |
| Codex | 委任範囲内の設計、実装、テスト、修正 | App Serverイベント、承認要求、Task Contract |
| GitHub Actions | リモート環境での再現可能な技術検証 | commit単位のcheck取得と証拠への関連付け |
| GitHub | PR、branch protection、mainへの統合履歴 | Integration Candidateのdraft PRと受け入れ後マージ |

GUIはこの連携全体のコントロールプレーンだが、各ツールが保持する詳細機能の代替ではない。

---

# 18. 推奨Rustワークスペース

```text
ai-game-control-plane/
├─ crates/
│  ├─ domain/
│  ├─ application/
│  ├─ project-documents/
│  ├─ event-journal/
│  ├─ scheduler/
│  ├─ codex-protocol/
│  ├─ codex-adapter/
│  ├─ git-adapter/
│  ├─ github-adapter/
│  ├─ conflict-analyzer/
│  ├─ build-adapter/
│  ├─ persistence/
│  ├─ desktop/
│  └─ cli/
│
└─ game-integration/
   ├─ acceptance-core/
   └─ bevy-acceptance-hud/  # MVP後の任意統合
```

依存方向：

```text
desktop ─┐
cli ─────┼→ application → domain
         │
         ├→ project-documents
         ├→ event-journal
         ├→ scheduler
         ├→ codex-adapter
         ├→ git-adapter
         ├→ github-adapter
         ├→ conflict-analyzer
         ├→ build-adapter
         └→ persistence
```

ドメイン層をCodex、GitHub、Dioxus、SQLiteへ直接依存させない。Task依存DAG、各ライフサイクル、Integration Candidate、Evidence、Acceptance Resultはdomain層に置く。実行枠、プロセス、外部サービスの状態はadapterまたはapplication層で扱う。

---

# 19. 実装マイルストーン

## Milestone 0：ドメインと文書

* Rust workspace作成
* Game Visionスキーマ
* Hypothesisスキーマ
* Backlogスキーマ
* Acceptance Criterionスキーマ
* Task Contractスキーマ
* Task依存DAGと循環検出
* Task、Task Run、Integration、Build、Acceptanceの状態機械
* Markdown front matterの読み書き
* 追記専用イベント履歴
* SQLite read modelの再構築

## Milestone 1：Project IntentとPlan Review

* Project Intent画面
* AIによる仕様・タスク分解
* Decision Card
* Task DAG
* 依存種別、依存理由、クリティカルパス
* 人間による承認
* Task文書生成
* 三つの変換を移動できるGUI shell

## Milestone 2：Codex実行

* App Server起動
* thread・turn管理
* worktree・branch作成
* Codexイベント取得
* Task Run画面
* Zedで開く操作
* QueueとScheduler
* 設定可能な並列実行上限
* スコープ逸脱検出

## Milestone 3：競合解析

* ファイル競合
* `syn`によるシンボル抽出
* 同一シンボル変更検出
* シグネチャ差分
* 型・trait・impl変更検出
* 未対応構文の`UNKNOWN`判定
* マージ順序提案
* Human Inbox連携

## Milestone 4：統合と品質ゲート

* Integration Candidate作成
* 依存関係に基づく統合順序
* 安全条件付き自動rebase
* GitHub Actions連携
* CI状態表示
* 独立AIレビュー
* Review Finding一覧
* 再試行上限付き自動修正
* draft PR作成
* 高リスク事項のHuman Inbox化

## Milestone 5：Buildと受け入れ

* Bevyビルド生成
* Build Registry
* ゲーム起動
* 受け入れ基準表示
* プレイ結果登録
* 受け入れ・再依頼・新規タスク・手動調整
* 受け入れ済みcommitのmainマージ
* commit変更時の受け入れ無効化

## Milestone 6：First Playableの一周

実際の粉遊びゲームで、企画から受け入れ、mainマージ、依存Taskの解除までの全フローを通す。

---

# 20. MVPでは作らないもの

* 独自コードエディタ
* 汎用ターミナル
* Zedの代替
* GitHub Actionsログビューアーの完全再実装
* Codexの全文チャットを中心としたUI
* 複数プログラミング言語のAST解析
* 完全な意味的競合証明
* rust-analyzerによる完全な参照解決
* 自動競合解消
* 完全自律マージ
* ゲーム内受け入れHUD
* 任意のゲームエンジン・プロジェクトへの汎用対応
* ストアへの自動公開
* 複数ユーザー・組織権限
* AIによるゲームの面白さの自動判定
* 高度なゲーム分析
* 正確な実装進捗率の推定

---

# 21. MVP完成シナリオ

次のシナリオを最初から最後まで実行できればMVP完成とする。

1. `vision.md`へ「砂を配置し、現象を観察する」という遊びの核を記録する
2. AIが仮説、受け入れ基準、設計案、依存関係付き実装タスクを生成する
3. 人間がDecision Card、Task DAG、並列実行計画を承認する
4. Task ContractがMarkdownとして生成され、依存DAGと許可パスが検証される
5. Schedulerが実行可能なTask RunをQueueから選び、設定された上限まで起動する
6. 各Task Runへbranchとworktreeが割り当てられる
7. 複数Codexが並列に実装とテストを行う
8. Local Checkとdiff検査がスコープ逸脱を検証する
9. Structural Conflict Analyzerがファイルとシンボルの競合を検出する
10. 依存関係と競合結果から統合順序を提案する
11. Task Runの成果をIntegration Candidateへ統合し、draft PRを作成する
12. baseが古い場合、安全条件を満たす範囲で自動rebaseして再検証する
13. GitHub ActionsがIntegration Candidateの技術的品質を検証する
14. 独立AIが実装をレビューし、修正可能な失敗は再試行上限内で自動修正する
15. 人間に必要な判断だけがHuman Inboxへ集約される
16. 技術検証済みのcommitからBevy Buildが生成される
17. 人間が実際に砂を配置して挙動を確認する
18. 問題があればAIへ再依頼し、新しいTask RunとBuildで再度プレイする
19. 人間がBuildを受け入れ、同一commitのPRを`main`へマージする
20. Task、バックログ、仮説の状態が更新され、依存していた後続Taskが解除される

---

# 22. MVPの完成条件

* 遊びの核からTaskまで追跡できる
* Codexが関連する制作文書を参照できる
* Task依存DAGを検証し、開始条件と統合順序へ反映できる
* Taskごとに変更可能範囲を制限できる
* 範囲外変更を実行者にかかわらず検出し、品質ゲートを停止できる
* 設定可能な実行上限のもと、複数Codexを個別worktreeで並列実行できる
* 実行枠を超えるTask RunをQueueで待機させられる
* Task Runの状態を一画面で確認できる
* CI失敗箇所をTaskと受け入れ基準へ関連付けられる
* 独立AIレビュー結果を一覧表示できる
* 同一シンボルを変更するTask Runを検出できる
* マージ順序を根拠付きで提案できる
* 安全なrebaseと上限付き自動修正を行い、判断不能時は人間へ戻せる
* 人間の判断事項をHuman Inboxへ集約できる
* Integration Candidateの一意なcommitから、検証済みのプレイ可能Buildを生成できる
* 人間が受け入れ、再依頼、新規タスク化、手動調整を選べる
* 受け入れたBuildと同一commitだけを`main`へマージできる
* commit変更時に古いCI、Build、Acceptance Resultを無効化できる
* GUI上でIntent to Work、Work to Evidence、Evidence to Decisionの一周を完了できる
* Markdownとイベント記録からSQLite read modelを再構築できる
* すべての判断と実行結果が履歴として残る
* 人間がCodexの全会話を巡回しなくても開発を進められる

---

# 23. MVPの評価指標

## 人間の負担

* Task当たりの人間介入回数
* Codexスレッドを直接開いた回数
* Human Inbox項目の処理時間
* プレイ可能ビルドまでの人間作業時間

## 並列開発

* 同時Task Run数
* Queue待ち時間と実行枠利用率
* 依存関係による待機時間
* 依存DAGの誤りによる再計画回数
* 競合の事前検出率
* マージ時に初めて発覚した競合数
* rebase後の再修正率

## 品質

* CI初回通過率
* AIレビュー指摘の的中率
* プレイ受け入れ時の不具合率
* マージ後不具合率
* スコープ逸脱率
* 受け入れ後のcommit変更による再受け入れ回数

## 開発速度

* Task開始からBuild生成までの時間
* Human Inbox待ち時間
* First Playable完成までの時間
* 修正依頼から再Buildまでの時間
* Build受け入れからmainマージまでの時間

---

# 24. MVPの本質

本MVPは、ダッシュボードやAIチャットを作るプロジェクトではない。

その本質は、次の三つの変換を再現可能にすることである。

```text
Intent to Work
遊びの意図を、実行可能なTask Contractへ変換する

Work to Evidence
AIの実装を、CI・レビュー・Buildという証拠へ変換する

Evidence to Decision
技術的成果を、人間が受け入れ判断し、mainへ統合できる形へ変換する
```

この三つの変換を、一貫したGUI上で理解・操作できることをMVPの中心成果とする。GUIは単なる監視ダッシュボードではなく、制作意図の承認、AIへの委任、証拠の評価、プレイ受け入れ、mainマージという状態遷移を安全に進める操作面である。

CLIは同じ変換を自動化・診断できる補助面として提供するが、MVPの主たる体験はGUIで成立させる。

Zedは各worktreeでコードを製造・確認・修正する。

GitHub ActionsはIntegration Candidateの技術的品質を検証し、GitHubは受け入れ済みcommitを`main`へ統合した履歴を保持する。

本システムは、複数worktreeを横断し、AIが何を作り、どこまで検証され、人間が何を判断すべきか、どの成果を`main`へ入れてよいかを管理する。

この責務境界を維持することが、MVPをZedの代替ではなく、AI並列ゲーム開発のコントロールプレーンとして成立させる条件である。
