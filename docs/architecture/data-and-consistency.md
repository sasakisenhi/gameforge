# データ、正本、整合性

> [アーキテクチャ概要](../../architecture.md)へ戻る

MarkdownとEvent Journalの正本、SQLite Projection、AST競合解析を定義する。

## 正本と一貫性モデル

### 情報種別ごとの正本

| 情報 | 正本 | SQLiteでの扱い |
| --- | --- | --- |
| Vision、原則、仮説、Backlog、Task Contract | Git管理Markdown | 検索・一覧用にProjection |
| Decision、Build metadata、Acceptance Result | 正規化Markdown | 関連・状態表示用にProjection |
| Task RunやIntegration Runの実行履歴 | 追記専用Event Journal | 最新状態と集計値へProjection |
| commit、branch、worktree | Git | 最終観測値と関連IDをProjection |
| PR、GitHub Actions、check | GitHub | commit付きEvidenceとしてProjection |
| Build本体、大容量ログ | 外部Artifact Store | URI、hash、生成元commitのみ保持 |

「Git管理Markdown」と「Event Journal」は役割の異なる正本である。人間が承認した宣言的な事実はMarkdown、時間とともに発生する実行事実はEvent Journalへ記録する。

### 整合性

単一プロセス内の巨大なトランザクションで、ファイル、Git、Codex、GitHubを同時に更新することはできない。そのため、外部副作用は次の二段階で扱う。

```text
Command
  → 純粋な事前条件判定
  → OperationRequestedイベントを追記
  → Workerが外部Portを実行
  → OperationSucceeded / OperationFailedイベントを追記
  → Read Modelを更新
```

各外部操作には安定した`operation_id`を付ける。Adapterは可能な範囲で冪等にし、再起動時には「要求済みだが結果未確定」の操作を外部状態と照合してから再実行する。

Markdown更新時は読み込み時のcontent hashまたはGit blob IDを前提条件にする。前提と現在値が異なる場合は上書きせず、同時編集としてHuman Inboxへ送る。

### イベントの最小エンベロープ

```text
event_id
schema_version
occurred_at
aggregate_type / aggregate_id
aggregate_version
correlation_id / causation_id
event_type
payload
actor
```

イベントは追記専用とする。訂正は既存イベントの更新ではなく、新しい訂正・無効化イベントで表現する。秘密情報、アクセストークン、大容量ログ本文はイベントへ格納しない。

## Structural Conflict Analyzer

競合解析は「各Task Runを振る舞いを持つオブジェクトとして相互呼び出しする」のではなく、解析対象を不変なデータ集合へ正規化し、パイプラインで処理する。

```text
Git diff
  → 変更ファイル集合
  → base/headのRust source snapshot
  → syn ASTからSymbolRecordを抽出
  → SymbolDeltaへ正規化
  → Task Run間でjoin
  → Rule集合でConflictFindingを生成
  → SAFE / POTENTIAL_CONFLICT / CONFLICT / UNKNOWN
```

主なデータは次のような平坦なrecordとする。

```text
SourceFile { path, blob_hash, language }
SymbolRecord { stable_key, kind, path, span, visibility, signature_hash, body_hash }
SymbolDelta { task_run_id, stable_key, change_kind, before_hash, after_hash }
ReferenceEdge { from_key, to_key, resolution }
ConflictFinding { left_run, right_run, rule_id, severity, evidence }
```

解析関数は入力snapshotを変更せず、新しいrecord集合を返す。ruleごとの判定を小さな純粋関数にし、検出理由を機械可読な`rule_id`とEvidenceで残す。

macro、生成コード、不完全な構文、名前解決不能は黙って除外せず`UNKNOWN`を生成する。`syn`と将来のrust-analyzerは同じ正規化recordを出力する別Adapterとして扱い、判定規則の全面書き換えを避ける。

## SQLite Read Model

### 方針

SQLiteはDomain Objectの保存先ではなく、画面とCLI Queryに最適化したProjection Storeである。第三正規形やAggregate構造の忠実な複製より、用途ごとの読み取りやすさを優先する。

Projection例:

- `development_board_rows`
- `task_run_details`
- `human_inbox_rows`
- `build_acceptance_rows`
- `backlog_progress_rows`
- `evidence_links`
- `scheduler_queue_rows`

実際のDDLは、Milestone 0のMarkdown実例と最初の画面Queryを作ってから確定する。

### Projection処理

```text
Markdown snapshot ─┐
                   ├→ Projector → SQLite transaction → revision更新
Event Journal ─────┘
```

- Projectorは入力順序とversionを検証する。
- 一つのイベント適用とcheckpoint更新を同一SQLite transactionで行う。
- `event_id`により重複適用を防ぐ。
- Projectionには元document revision、aggregate version、対象commitを保持する。
- schema変更時はmigrationだけでなく、空DBからのrebuildを検証する。
- SQLite破損時に正本を失わず、削除・再生成できることを完成条件とする。

一覧の総合状態や進捗はSQLまたはProjectorで導出し、Domainへ書き戻さない。

