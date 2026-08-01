# Runtime、Scheduler、GUI

> [アーキテクチャ概要](../../architecture.md)へ戻る

Scheduler、Dioxus GUI、非同期処理、単一Writer、DesktopとCLIの競合、代表フローを定義する。

## Scheduler

Schedulerは、純粋な計画ロジックと、実行資源の管理を分ける。

### Scheduling Policy: Functional Core

```rust
fn plan(input: &ScheduleSnapshot, config: &ScheduleConfig)
    -> SchedulePlan;
```

`ScheduleSnapshot`は少なくとも次を含む。

- Queue内のTask Runと優先度
- `blocks_start`依存の充足状況
- 承認待ち、入力待ち、stale、scope violation
- allowed pathと構造競合リスク
- 現在のresource lease
- CPU、メモリ、process数の利用可能量
- `max_concurrent_task_runs`

`SchedulePlan`は`start`、`keep_running`、`defer`と、各判断理由を返す。純粋関数はprocessを起動せず、worktreeも変更しない。

同じ優先度では安定した規則、たとえば「依存上のクリティカル度、queue投入時刻、Task Run ID」の順で決め、再現可能にする。競合情報が不足している場合は安全側へ倒し、`defer`またはHuman Inboxを選ぶ。

### Run Supervisor: Imperative Shell

`RunSupervisor`は次の実行資源をオブジェクトとして所有する。

- Codex child processとstdio
- thread / turn session
- cancellation token
- resource lease
- worktree lease
- event購読task
- 再接続・終了処理

SupervisorはSchedulePlanを受け、Operation Eventを記録してからAdapterを呼ぶ。resource leaseの獲得に失敗した場合はTask Runを失敗扱いせずQueueへ戻す。

並列上限を下げても実行中Runは暗黙に停止しない。新規開始を抑制し、明示的な停止Commandがある場合だけ中断する。

### 再起動時のReconciliation

起動時にEvent Journalと外部状態を照合する。

- 記録上実行中だがprocessが存在しないRunを検出する。
- worktree、branch、HEAD、未記録変更を確認する。
- Codex threadを再開可能か確認する。
- 結果不明のOperationを成功・失敗と推測せず、照合結果をEvent化する。
- 人間の未記録変更があるworktreeを自動rebase、削除、上書きしない。

## Dioxus GUI

GUIは状態駆動で、表示状態と副作用を分離する。

```text
Read Model subscription / Query
          ↓
      App View State
          ↓
     Dioxus Component
          │ User Intent
          ↓
   Application Use Case
          ↓
 Event / Projection更新
          └──────────────→ 再描画
```

### UI State

UI stateは次の三種類に分ける。

- Server/Domain由来: Read Modelのsnapshot、revision、更新時刻
- 操作中の一時状態: 選択、filter、入力フォーム、dialog
- 非同期操作状態: idle、submitting、succeeded、failed、再試行可否

Domain由来の状態をComponentローカルへ複製して正本化しない。一覧はIDで正規化し、選択中要素はIDで参照する。Projection revisionが変わった場合、古いフォームからの更新は期待versionで検出する。

### Componentの責務

- ComponentはPortやAdapterを直接保持しない。
- Componentから`git`、Codex JSON-RPC、GitHub API、SQLを直接呼ばない。
- Event handlerはユーザーの意図をUse Case Commandへ変換する。
- Reducerは可能な限り純粋にし、描画中に副作用を起こさない。
- 長時間操作はoperation IDで追跡し、二重送信を防ぐ。
- 主要操作の前に対象、根拠、影響、次の遷移を表示する。
- 詳細コード、diff、ログはZedまたはGitHubへのdeep linkを使う。

画面間で独自の状態解釈を持たず、Development Board、Human Inbox、Build & Acceptanceは共通のProjection DTOと状態表示規則を利用する。

## 非同期処理と実行モデル

### Runtimeとworker

- Tokioは`runtime`のworker、I/O Adapter、`bootstrap`、subscriptionで使用する。
- Domain、ApplicationのUse Case/Port、純粋なScheduler PolicyはTokioへ依存しない。
- Aggregate単位のCommand処理はversion検査により直列化する。
- 外部イベントは境界で内部Eventへ正規化してから処理する。
- cancellationは「要求」と「実際に停止を観測した事実」を分ける。
- timeoutは業務上の失敗と通信上の不確定を分ける。
- retryはAdapterの一時障害に限定し、設計判断や検査失敗を無条件に再試行しない。

バックグラウンドworkerの候補は次のとおりである。

- Scheduler tick / queue再評価
- Codex event collector
- Git workspace reconciler
- GitHub check pollerまたはwebhook consumer
- Projection builder
- Build runner
- stale evidence detector

worker同士は共有可変状態を直接操作せず、Command、Event、operation IDで連携する。

### プロセス構成

MVPでは、プロジェクトごとに一つの`ProjectCoordinator`がMutation Ownerとなる。Coordinatorは論理的な役割であり、必ずしも常駐daemonという独立processを意味しない。Desktop processへ内包する場合と、CLIからheadlessに起動する場合のどちらでも同じ責務と排他規則を適用する。

```text
Desktop process ─┐
                 ├─ LocalControlChannel ─→ ProjectCoordinator
CLI process ─────┘                           │
                                             ├─ Command Handler / Process Manager
                                             ├─ Event Journal Appender
                                             ├─ SQLite Projector Writer
                                             ├─ Scheduler / Run Supervisor
                                             └─ Git / Codex / GitHub / Build Adapters

ProjectCoordinator
  ├─ Codex child processes
  ├─ Git and Build child processes
  └─ asynchronous workers
```

`LocalControlChannel`は、同一process内では直接呼び出し、別processからはversion付きのlocal IPCとして実装できる抽象境界である。通信方式、endpoint形式、encodingはADRで決めるが、DesktopとCLIが利用するCommand、Query、結果の意味は同一にする。

役割は次のように分ける。

| 役割 | 責務 | 禁止事項 |
| --- | --- | --- |
| Desktop Client | View表示、User Intentの送信、Query購読 | 正本、SQLite、Git、Codexへ直接writeしない |
| CLI Client | Command/Query送信、結果の人間向け/JSON表示 | 正本やAdapterをCoordinator外から直接変更しない |
| ProjectCoordinator | Command処理、Writer所有、workerと外部操作の調停 | leaseなしでMutationを開始しない |
| Child Worker/Adapter | 割り当てられた外部I/Oを実行し結果を返す | 正本状態を独自判断で更新しない |

### 単一Writer

単一Writerの単位はアプリケーション全体ではなく、正規化されたproject rootごととする。異なるプロジェクトは別々のCoordinatorで並行実行できる。

CoordinatorはMutation開始前に排他的な`ProjectWriterLease`を取得する。leaseはOSがprocess終了時に解放できる排他lockを基礎とし、project ID、coordinator instance ID、process情報、protocol version、local endpointを発見用metadataとして持つ。metadataだけを根拠に所有権を判断せず、排他lockの取得結果を正とする。

| 変更対象 | Writer | 読み取り |
| --- | --- | --- |
| Event Journal | Coordinator内の単一Appender | Projector、診断処理 |
| SQLite Read Model | Coordinator内の単一Projector Writer | Coordinator経由のDesktop/CLI Query |
| Markdown文書 | Coordinatorの`ProjectDocumentStore` | Coordinator、Zedなど外部editor |
| Git branch/worktree/rebase | Coordinatorの`GitWorkspace` | Coordinator、Zedによる外部観測 |
| Codex/GitHub/Build操作 | Coordinatorが発行するoperation | Clientは状態だけをQuery |

Zedや人間が直接実行したGit commandなど、Control Plane外の変更までlockで禁止することはできない。外部変更はcontent hash、Git HEAD、worktree statusとの比較で検出し、Coordinatorが無断で上書きせずReconciliationまたはHuman Inboxへ送る。

SQLite自体のlock機能やEvent Journalのatomic appendだけに多重Writerの調停を任せない。すべてのMutationをCoordinatorへ通すことで、Markdown、Journal、SQLite、Git操作をまたぐ業務上の順序を一か所で管理する。

### Commandの直列化と冪等性

DesktopとCLIから送るCommandは少なくとも次を持つ。

```text
CommandEnvelope {
  command_id,
  project_id,
  client_id,
  expected_aggregate_version,
  issued_at,
  payload
}
```

- Coordinatorは`command_id`で受理済みCommandを重複排除する。
- AggregateごとのCommandは`expected_aggregate_version`を検査して直列化する。
- Event Journalへのappend順序はCoordinatorの単一Appenderが確定する。
- Clientがtimeout後に再送する場合は、同じ`command_id`を使用する。
- 外部副作用のexactly-onceを仮定せず、`operation_id`による冪等実行とReconciliationを使用する。
- 競合したCommandはlast-write-winsで上書きせず、`VersionConflict`として最新revisionとともにClientへ返す。

### DesktopとCLIの競合処理

DesktopとCLIは次の手順でCoordinatorを発見・起動する。

1. project rootを正規化し、既存Coordinatorのendpointを探索する。
2. endpointへ接続でき、protocol互換性があればClientとして利用する。
3. Coordinatorが見つからない場合だけ、`ProjectWriterLease`の取得を試みる。
4. lease取得に成功したprocessがCoordinatorを起動し、Adapterとworkerを構成する。
5. lease取得に失敗したprocessはWriterを起動せず、endpointを再探索してClientになる。

DesktopとCLIが同時起動した場合、排他leaseを取得した一方だけがWriterになる。敗者は正本、SQLite write connection、Git/Codex Adapterを開かない。既存Coordinatorへ接続できないがlockが生きている場合、強制的にlockを奪わず`CoordinatorUnavailable`として表示する。

Desktop終了後にCLIを実行する場合、CLIはleaseを取得して一時的なheadless Coordinatorを起動できる。長時間のTask Runを開始した場合、Coordinatorはactive operationを安全な状態へ進めるか明示的に中断するまで生存し、CLI process終了によって暗黙にworkerを孤児化させない。常駐化やdetachの具体的UXはADRで決める。

ClientとCoordinatorのprotocol versionが非互換の場合、Mutationを拒否する。互換性のない古いCLIが新しいCoordinatorへ直接書き込むfallbackは設けない。

Queryも原則としてCoordinator経由とし、ClientがSQLite schemaやProjection revisionへ依存しないようにする。診断用の直接read-only accessを追加する場合も、正本やcheckpointを更新できない別modeとして明示する。

### 終了と復旧

Coordinatorは終了時に新しいCommand受付を停止し、進行中Operationの状態をEvent Journalへ記録し、Projector checkpointを確定してからleaseを解放する。強制終了ではOS lockの解放後、次のCoordinatorが次を実行する。

- Event Journal末尾とschema/versionを検証する。
- 未完了のOperationRequestedを外部状態と照合する。
- SQLite checkpointがJournalより遅れていればEventを再適用する。
- SQLiteが再利用不能なら正本から再構築する。
- Git HEAD、worktree、Codex process/threadを再照合する。
- 復旧が確定するまで新規Mutationを受理しない。

## 代表的な処理フロー

### Task Run開始

```text
1. Desktop/CLI ClientがQueueTaskRunのCommandEnvelopeを送る
2. CoordinatorがWriter lease、command_id、expected versionを検証する
3. ApplicationがTask Contract revisionとbase commitを固定する
4. DomainがTask状態、DAG、scope定義を検証する
5. Coordinatorの単一AppenderがTaskRunQueuedをEvent Journalへ追記する
6. Scheduler Policyが依存・競合・容量を評価する
7. RunStartRequestedとresource leaseを記録する
8. Supervisorがbranch/worktreeとCodex threadを作成する
9. production pathをロックしてRed turnを開始する
10. 失敗理由とworktree snapshotをRed証拠として検証する
11. production pathを解放してGreen turnを開始する
12. Green確認後にRefactor turnと最終suiteを実行する
13. TddCycleEvidenceと外部結果を記録する
14. ProjectorがBoardを更新する
```

途中で失敗した場合、完了済みの外部操作を推測で巻き戻さない。観測結果を記録し、安全な補償処理または人間の判断へ進める。

### Integrationから受け入れ・merge

```text
Task Runs
  → TDD証拠・最終Greenの検証
  → 依存順・競合判定
  → Integration Candidate commit
  → draft PR
  → commit一致のCI + AI Review
  → Build生成
  → 同一commitを人間がplaytest
  → Acceptance Result
  → merge直前のcommit/check再照合
  → mainへmerge
  → Task受け入れ・後続Task再評価
```

どこかでcommitが変わった場合は`EvidenceInvalidated`を発生させ、技術検証以前へ戻す。UI上の受け入れ表示だけを残して強制mergeしてはならない。

