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

## Desktop

サンプルプロジェクトのRead Modelを再構築し、Development Boardをネイティブウィンドウで開きます。

```console
cargo run -p gameforge-desktop -- examples/powder-game
```

画面内のナビゲーション、Taskフィルター、行選択はローカルUI状態として扱います。QueueボタンはApplication層の`ApplicationCommand`へ変換され、Project Sessionが`TaskRunQueued`をEvent Journalへ記録してRead Modelと画面を更新します。同じcommand IDの再送は重複記録せず、古いProjection revisionからの操作は変更前に拒否します。

現在の実行容量表示は1です。Queue登録後にworktreeやCodexを起動するSchedulerは次の縦切りで接続するため、この段階ではTask Runは`QUEUED`に留まります。

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
└── runtime/
```

## 開発時の確認

```console
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build --workspace
```
