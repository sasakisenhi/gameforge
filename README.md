# gameforge

AI並列ゲーム開発コントロールプレーンの実装です。設計資料のMilestone 0と、Dioxus Nativeによる最初のデスクトップ縦切りを実装しています。

## 実装済み

- `Task`、`TaskRun`、`Integration Candidate`、`Build`、`Playtest Session`、`Acceptance Result`の状態と不変条件
- Task依存DAGの参照・自己依存・循環検査と決定的な統合順序
- Candidate、CI、Review、Build、Acceptance、PR headのcommit整合性を確認するmerge gate
- YAML front matter付きTask Markdownのschema、scope path、依存関係検証
- aggregate versionとevent IDを検証する追記専用JSONL Event Journal
- MarkdownとEvent Journalから再構築できるSQLite Development Board projection
- project root単位のOS Writer lockとprotocol version検査
- 共通bootstrapを使用するheadless CLI
- Application層が所有する型付きView DTOとrevision付きCommand変換
- Dioxus NativeのApplication Shell、Development Board、切断時の読み取り専用表示
- GUI起動中にWriter Leaseを保持するProject Session
- Queue commandのEvent Journal永続化、冪等な再送、Projection競合検出
- Run取消しのEvent Journal永続化、再送冪等性、取消後の再Queue
- Agent入力要求のEvent Journal永続化とInbox Read Model
- Inbox回答のEvent Journal永続化とAgent Run再開
- 決定的な優先順と設定可能な実行上限（既定3）を守るScheduler Policy
- 外部起動前のOperation記録と、lease取得結果を扱うRun Supervisor境界
- Task RunごとのGit worktree作成とCodex App Server起動を行う実`RunExecutionPort`
- Codexの完了・失敗・入力要求とRun取消しを監視し、空いた実行枠を補充するCoordinator worker

## Desktop

サンプルプロジェクトのRead Modelを再構築し、Development Boardをネイティブウィンドウで開きます。
Taskを実行するには、認証済みの`codex` CLIが`PATH`上に必要です。

```console
cargo run -p gameforge-desktop -- examples/powder-game
```

並列上限は既定で3です。正の整数を環境変数で指定できます。

```console
GAMEFORGE_MAX_CONCURRENT_TASK_RUNS=5 cargo run -p gameforge-desktop -- examples/powder-game
```

画面内のナビゲーション、Taskフィルター、行選択はローカルUI状態として扱います。QueueボタンはApplication層の`ApplicationCommand`へ変換され、Project Sessionが`TaskRunQueued`をEvent Journalへ記録します。続けてSchedulerがQueueを評価し、容量が空いていれば最初のTask Runを`PREPARING`へ進めてRead Modelと画面を更新します。同じcommand IDの再送は重複記録せず、古いProjection revisionからの操作は変更前に拒否します。

進行中RunにはCancel Run操作を表示します。取消しは`TaskRunStateChanged`の`CANCELLED`として永続化され、同じcommand IDの再送では重複しません。取消し後は現在Run IDを解放するため、同じTaskを新しいattemptとしてQueueできます。

Agentから入力が必要になったRunは`INPUT_REQUIRED`へ遷移し、要求ID、質問文、Task、Run、受付時刻をInboxへ表示します。InboxはSQLiteだけに依存せずEvent Journalから再構築でき、同じ入力要求commandの再送でも項目を重複させません。

`PENDING`の入力要求にはInboxから回答できます。空回答、切断中、古いProjection、解決済み要求は送信前に拒否します。回答はEvent Journalへ記録され、Inbox itemを`ANSWERED`へ更新すると同時にRunを`AGENT_RUNNING`へ戻します。

Run Supervisorは外部Adapterを呼ぶ前に`TaskRunStartRequested`を記録し、Git worktreeとCodex threadを実際に作成できた場合だけ`AGENT_RUNNING`へ進めます。Task Contractをpromptへ含め、各worktree内でCodex App Serverの`thread/start`と`turn/start`を実行します。Codexは`workspace-write`、network無効、承認要求なしで起動します。

Coordinator workerは250ms間隔でCodexの通知を監視します。turn完了時はRunを`LOCAL_CHECKING`、異常終了時は`FAILED`へ進め、取消時はCodex processを停止します。いずれも実行枠を解放した直後にQueueを再評価するため、待機中のRunが自動的に開始されます。資源を確保できない場合はRunを失敗扱いせず`QUEUED`へ戻し、後から再スケジュールします。worktreeは取消し・完了後も成果物確認と後続統合のため保持します。

実行ログは`.game-dev/runtime/runs/<run-id>/`、worktreeは`.game-dev/worktrees/<run-id>/`に作成します。Codex turn後のローカルcheck実行と統合処理は、この縦切りの次段階です。

## CLI

サンプルプロジェクトのTask文書を検証します。

```console
cargo run -p gameforge-cli -- validate examples/powder-game
```

Event JournalからSQLite Read Modelを再構築し、Development Boardを表示します。

```console
cargo run -p gameforge-cli -- rebuild examples/powder-game
```

`rebuild`は次のローカル生成物を作ります。SQLiteとruntime lock metadataはGit管理しません。

```text
examples/powder-game/.game-dev/
├── events/events.jsonl
├── read-model.sqlite
├── runtime/
│   └── runs/<run-id>/
│       ├── codex-events.jsonl
│       └── codex-stderr.log
└── worktrees/<run-id>/
```

## 開発時の確認

`x86_64-unknown-linux-gnu`ではDesktopリンク時のメモリ使用量と待ち時間を抑えるため、`.cargo/config.toml`で`clang + mold`を使用します。Linuxで開発する場合は`clang`と`mold`が必要です。

```console
mold --version
clang --version
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -j 1 -- -D warnings
cargo test --workspace -j 1
cargo build --workspace -j 1
```
