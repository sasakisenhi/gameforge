# gameforgo

AI並列ゲーム開発コントロールプレーンの実装です。現在は設計資料のMilestone 0と、最初のCLI縦切りを実装しています。

## 実装済み

- `Task`、`TaskRun`、`Integration Candidate`、`Build`、`Playtest Session`、`Acceptance Result`の状態と不変条件
- Task依存DAGの参照・自己依存・循環検査と決定的な統合順序
- Candidate、CI、Review、Build、Acceptance、PR headのcommit整合性を確認するmerge gate
- YAML front matter付きTask Markdownのschema、scope path、依存関係検証
- aggregate versionとevent IDを検証する追記専用JSONL Event Journal
- MarkdownとEvent Journalから再構築できるSQLite Development Board projection
- project root単位のOS Writer lockとprotocol version検査
- 共通bootstrapを使用するheadless CLI

## CLI

サンプルプロジェクトのTask文書を検証します。

```console
cargo run -p gameforgo-cli -- validate examples/powder-game
```

Event JournalからSQLite Read Modelを再構築し、Development Boardを表示します。

```console
cargo run -p gameforgo-cli -- rebuild examples/powder-game
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

