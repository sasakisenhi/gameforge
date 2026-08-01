# Applicationと外部連携

> [アーキテクチャ概要](../../architecture.md)へ戻る

Ports and Adapters、外部依存の局所化、技術スタック、Rust workspaceの依存方向を定義する。

## Application層とPorts and Adapters

### Application層の責務

- GUI/CLIからCommandまたはQueryを受け取る。
- 必要なAggregateと正本データをPort経由でロードする。
- Domainの純粋な判定を呼び出す。
- Event追記と外部Operationを調停する。
- 長時間処理をProcess Managerとして進める。
- 認証エラー、通信エラー、検査失敗、ドメイン拒否を区別する。

Application層はGitコマンド文字列、Codex JSON-RPCのversion差、GitHub API payload、SQLを知らない。

### Inbound Ports

GUIとCLIへ、同じUse Caseを型付きAPIとして公開する。

例:

- `ApprovePlan`
- `QueueTaskRun`
- `CancelTaskRun`
- `AnswerDecisionRequest`
- `ComposeIntegrationCandidate`
- `RequestVerification`
- `RequestBuild`
- `StartPlaytest`
- `RecordAcceptanceResult`
- `MergeAcceptedCandidate`
- `RebuildReadModel`

Commandは対象ID、期待version、操作者、理由を持つ。QueryはRead Model専用Portを通し、Aggregateを一覧表示用に大量ロードしない。

### Outbound Ports

Portは外部製品名ではなく、Applicationが必要とする能力で定義する。

| Port | 主な能力 | Adapter例 |
| --- | --- | --- |
| `ProjectDocumentStore` | 文書load、検証、条件付きwrite | Markdown / filesystem |
| `EventJournal` | append、aggregate stream読込 | JSONL等の追記専用記録 |
| `GitWorkspace` | diff、branch、worktree、rebase、commit | Git CLI |
| `AgentRuntime` | thread/turn開始、停止、event購読 | Codex App Server |
| `LocalVerification` | worktreeのscope・品質gate実行、取消し、結果取得 | Git / Cargo process |
| `CodeReviewService` | 独立レビュー要求、結果取得 | 別Codex session |
| `RemoteVerification` | PR/check/runの要求・取得 | GitHub / Actions |
| `ArtifactBuilder` | commitからBuild生成 | Cargo / Bevy build |
| `ArtifactStore` | artifact保存、hash、URI取得 | ローカルfilesystem等 |
| `ConflictAnalyzer` | 変更集合の構造解析 | `syn`ベース解析器 |
| `ReadModelStore` | Projection更新、Query | SQLite |
| `EditorLauncher` | worktree、file、lineを開く | Zed CLI |
| `Clock` / `IdGenerator` | 時刻とIDを供給 | system / test実装 |

PortのtraitはApplication側に置き、Adapterが実装する。外部ライブラリ固有型をPortの引数・戻り値へ漏らさず、境界DTOへ変換する。

### Adapterのライフサイクル

Adapterは、接続、process handle、認証、rate limit、再試行、protocol versionなどの可変な実行資源をカプセル化する。内部に可変状態を持てるが、その状態をDomain上の事実と混同しない。

破壊的Git操作やmergeでは、実行前後のHEAD、対象worktree、operation IDを検証・記録する。Adapterが自動的に対象範囲を広げてはならない。

### 外部依存の局所化

本書でいう「外部ツールに依存するクラス」は、Rustにおける外部crate、wire protocol、CLI、OS processへ依存する`struct`、`enum`、moduleを指す。これらの公開範囲と個数を最小限にする。

外部システムごとに、Application Portを実装する公開Facadeを原則一つ置く。

```text
Application Port / 内部DTO
             ▲
             │ implement / translate
┌────────────┴────────────────────┐
│ public Adapter Facade           │  外部依存を知る公開型は原則ここだけ
│  ├─ private client/session      │
│  ├─ private wire DTO mapper     │
│  ├─ private retry/reconcile     │
│  └─ private process/resource    │
└────────────┬────────────────────┘
             │
      外部SDK / CLI / protocol
```

「一つのFacade」は、一つの巨大な`ExternalTools`や万能Adapterへ全連携を集約するという意味ではない。Git、Codex、GitHub、SQLiteなどの変更理由が異なる境界は別crate・別Facadeとし、各Facade内部を小さな非公開型へ分割する。

次の規則を守る。

- CodexのJSON-RPC DTO、GitHub SDKの型、SQL row、Dioxusのsignal、Tokioのhandle、`syn`のAST nodeをPort、Domain Entity、View DTOへ含めない。
- 外部入力はAdapter入口で検証し、内部の安定したDTO、Value Object、Eventへ即座に変換する。
- 外部製品のAPI構造をそのまま模したPortを作らず、Applicationが必要とする最小の能力でPortを定義する。
- 外部crateの型を`pub use`せず、FacadeのconstructorとPort実装以外は原則`pub(crate)`またはprivateにする。
- protocol versionの分岐、互換処理、既定値補完をDomainやGUIへ持ち込まず、protocol/Adapter内で吸収する。
- raw payloadが診断に必要な場合は、Domain Eventへ埋め込まず、redactしたログのURI/hashをEvidenceから参照する。
- 外部ツールをfake化するためだけの条件分岐をDomainへ追加せず、同じPortを実装するfake Adapterへ差し替える。

### 安定した境界DTOとエラー

内部DTOは画面や外部APIの都合ではなく、Use Caseに必要な情報だけを持つ。外部にフィールドが追加されても内部で利用しなければDTOを変更しない。外部フィールドのrename、列挙値追加、nullable化はMapperが吸収し、未知の値は情報を失わない`Unknown`または明示的なprotocol errorへ変換する。

外部ライブラリ固有のerror型もPortから返さない。少なくとも次の情報へ正規化する。

```text
ExternalFailure {
  system,
  operation,
  kind,
  retryability,
  result_certainty,
  diagnostic_ref
}
```

`result_certainty`は、外部操作が未実行、失敗確認済み、成功確認済み、結果不明のどれかを表す。これにより、ライブラリ更新でerror階層が変わってもApplicationの復旧フローを変更せずに済む。

## 技術スタック

技術スタックは単なる実装ライブラリの一覧ではなく、プロセス境界、整合性モデル、復旧方法、テスト可能性を決めるアーキテクチャ上の選択として扱う。

### 採用技術と影響範囲

| 技術 | 主な役割 | アーキテクチャへの影響 |
| --- | --- | --- |
| Rust | システム全体の実装言語 | 型安全性、状態機械、プロセス管理、AST解析、crate間の依存境界を決める |
| Dioxus Desktop | デスクトップGUI | GUIの実行方式、Componentの責務、Application層とのフロントエンド境界を決める |
| Tokio | 非同期runtime | Schedulerの駆動、Codex/Git/GitHub/Buildの並行I/O、process監視と停止処理を決める |
| Markdown + Event Journal | 宣言的事実と実行履歴の保存 | 正本、schema version、同時編集検出、履歴と復旧の整合性モデルを決める |
| SQLite | 再構築可能なRead Model | 一覧・検索・集約のデータ形状、Projection、migration、rebuild方式を決める |
| Codex App Server | AI実行runtime | AI実行のprocess境界、stdio JSON-RPC、thread/turn、承認・入力待ちの扱いを決める |
| Git CLI | Git操作の実装 | Git Adapterのコマンド境界、worktree/branch/rebase操作、障害・部分成功の扱いを決める |
| GitHub Actions | リモート技術検証 | commit単位の技術検証の正本、required check、Build前の品質ゲートを決める |
| `syn` | Rust構文解析 | MVPのシンボル抽出能力と、macro展開・名前解決を含まない解析限界を決める |
| Bevy | 対象ゲームruntime | Build生成、ゲームprocess起動、artifact管理、プレイ受け入れの対象境界を決める |

### 技術ごとの採用方針

#### Rust

Domain、Application、Adapter、GUI、CLIを同一Rust workspaceで構成する。Task IDやcommit SHAなどを単なる`String`として使い回さず、用途ごとのnewtypeで誤接続を防ぐ。状態機械とEvidenceの有効性をenum、Value Object、`Result`で表現し、不正な組み合わせをコンパイル時またはDomain境界で拒否する。

Rustを採用していても、process handle、socket、channel、DB connectionをDomain型へ含めない。これらはApplicationのruntimeまたはAdapterが所有する。

#### Dioxus Desktop

Dioxus DesktopはMVPの主操作面を提供する。Desktop process内でApplication Use Caseを呼び出せる構成にするが、ComponentとApplicationの間には型付きCommand、Query、View DTOの境界を置く。

Dioxus固有のsignal、hook、component型をApplicationやDomainへ公開しない。ウィンドウ、webview、ファイルdialog、deep linkなどデスクトップ固有機能は`desktop` crate内へ閉じ込める。将来CLIや別UIを追加してもUse Caseを再利用できることを条件とする。

#### Tokio

Tokio runtimeは共通`bootstrap`から起動し、Desktopへ内包されたCoordinatorまたはheadless Coordinatorとして`runtime` workerとI/O Adapterを実行する。ApplicationのUse CaseとPortはruntime非依存に保つ。主な非同期処理の対象はCodex child process、Git command、GitHub通信、Build process、Projection更新、Schedulerの再評価である。

- DomainとSchedulerの`plan`関数は同期的な純粋関数のままにする。
- channelは有界を基本とし、過負荷を無制限なqueue増加へ変えない。
- 非同期taskには所有者と終了条件を持たせ、切り離されたtaskを作らない。
- lockを保持したまま外部I/Oを`await`しない。
- cancellation要求、processへの停止通知、process終了の観測を別のEventとして記録する。
- timeout後の外部状態を失敗と断定せず、結果不明としてReconciliation対象にする。

#### Markdown + Event Journal

Markdownは人間が読み書きし、Gitでレビューする宣言的な正本にする。YAML front matterへ安定したID、`schema_version`、関連ID、状態、revision情報を持たせる。本文は説明、理由、非目標など、人間向けの文脈を保持する。

Event JournalはTask Run、Integration Run、外部Operationなど、時間順に発生する事実を追記専用で記録する。文書の現在値と実行履歴を一つの保存形式へ無理に統合しない。両者をProjectorが読み取り、SQLiteへ一貫した表示状態を構築する。

#### SQLite

SQLiteはローカルのProjection Storeであり、制作意図や受け入れ結果の唯一の保存先にしない。書き込みはProjectorへ集約し、GUI Componentや個別Adapterから任意のSQL更新を行わない。Query側は画面単位のDTOを返し、DB rowをDomain Entityとして公開しない。

schema migrationに加えて、空DBからのfull rebuildを常にサポートする。逐次更新したDBとfull rebuildしたDBが同じ論理結果になることをテストする。DB lock、disk full、破損は正本の喪失ではなく、Projection停止・再構築可能な障害として扱う。

#### Codex App Server

Codex App Serverはローカルchild processとして起動し、stdio上のJSON-RPCで通信する。`codex-protocol`はwire DTO、method、notification、protocol versionを扱い、`codex-adapter`はそれらを`AgentRuntime` Portの内部型とEventへ変換する。

thread、turn、approval request、user input request、command execution、file change、終了状態をTask Runへ関連付ける。process終了、壊れたJSON、応答timeout、protocol非互換をTaskの実装失敗と区別する。Task Runごとにprocessを分けるか共有するかはAdapter内部の方針とし、Domain Modelへ固定しない。

#### Git CLI

MVPではGitライブラリではなくGit CLIを`GitWorkspace` Adapterから呼び出す。コマンドはshell文字列の連結ではなく、実行ファイル、引数、明示的なworking directoryとして組み立てる。可能な限りmachine-readableで安定した出力を使用し、終了code、stdout、stderrを区別して記録する。

branch、worktree、rebase、diff、commitを実行する前にrepository root、対象ref、HEAD、worktree状態を検証する。操作後にも同じ項目を再取得し、期待結果との一致をEvent化する。競合、認証、lock、未記録変更、対象ref消失、command timeoutを別の障害として扱い、部分成功した操作を自動的に成功または失敗と決めつけない。

#### GitHub Actions

GitHub ActionsはIntegration Candidateに対するリモート技術検証の正本である。本システムはworkflow engineを再実装せず、GitHub Adapterを通して実行を要求・観測し、workflow、run、job、check、attempt、commit SHAをEvidenceへ関連付ける。

required checkが成功していても、対象commitが現在のIntegration Candidateと異なれば無効とする。Actionsの通信障害、認証障害、rate limitは検査失敗と区別する。GitHub Actionsが利用不能でもローカル実装は継続できるが、技術検証済みへの遷移、Build受け入れ、`main` mergeは保留する。

#### `syn`

`syn`はMVPにおけるRust sourceのAST parserとして使用する。関数、構造体、enum、trait、`impl`、visibility、signatureなどを`SymbolRecord`へ正規化し、Task Run間の差分を比較する。

`syn`単体ではmacro展開後の意味、完全な名前解決、型推論、動的な参照関係を保証できない。この範囲外を「変更なし」や`SAFE`にせず、解析coverageと`UNKNOWN`を結果へ含める。将来rust-analyzerを追加しても、Conflict Ruleは共通の正規化recordに対して動作させる。

#### Bevy

Bevyはコントロールプレーン自身のGUI frameworkではなく、MVPで生成・起動・プレイ受け入れするゲーム側のruntimeである。`build-adapter`は対象repositoryが定義したbuild commandを実行し、生成元commit、platform、artifact hash、起動方法をBuild metadataへ記録する。

Domain、Application、Dioxus GUIをBevyへ依存させない。Bevy processの起動・終了・crashは`ArtifactBuilder`またはゲーム起動用Adapterの境界で扱う。ゲーム内HUDはMVP必須にせず、GUIが対象Buildと受け入れ基準を表示してゲームを起動し、終了後にPlaytest Sessionを記録する。

### Versionと交換可能性

正確なtoolchainとcrate versionは`rust-toolchain.toml`、各`Cargo.toml`、lockfileを正本とし、本書へ重複記載しない。主要version更新では、次を確認する。

- Dioxus Desktopの起動方式、非同期連携、platform差分
- Tokioのprocess、channel、shutdown動作
- Codex App Serverのprotocol互換性
- Git CLIの最低対応versionと出力形式
- GitHub Actions/APIのcheck状態対応
- `syn`が生成するASTと解析coverage
- Bevyのbuild artifact、起動引数、対象platform

技術固有型をAdapterまたはUI境界へ閉じ込め、変更時にはPort contract test、schema compatibility test、First Playableのend-to-end testで影響を検証する。

外部技術の更新では、最初にprotocol/Adapterとfixtureだけを更新する。Port contractを変えずに全testが通るなら、Domain、Application、GUIは変更しない。Portの変更が必要なのは、外部APIの形が変わったときではなく、本システムが必要とする能力または業務上の意味そのものが変わったときだけとする。

## Rust Workspaceと依存方向

```text
crates/
├─ domain/                 # Entity、Value Object、状態機械、不変条件
├─ application/            # Use Case、Port、Process Manager
├─ control-protocol/       # Local Command/Query DTO、version handshake
├─ project-documents/      # Markdown schemaとProjectDocumentStore adapter
├─ event-journal/          # EventJournal adapter
├─ scheduler/              # 純粋なScheduling Policy
├─ runtime/                # ProjectCoordinator、Tokio worker、Run Supervisor
├─ codex-protocol/         # JSON-RPC wire DTOとcodec
├─ codex-adapter/          # AgentRuntime implementation
├─ local-check-adapter/    # LocalVerification implementation
├─ git-adapter/            # GitWorkspace implementation
├─ github-adapter/         # RemoteVerification implementation
├─ conflict-analyzer/      # 解析record、pipeline、ConflictAnalyzer implementation
├─ build-adapter/          # ArtifactBuilder implementation
├─ persistence/            # SQLite ProjectorとQuery implementation
├─ bootstrap/              # Writer lease取得と具体Adapterの共通composition
├─ desktop/                # Dioxus ClientとCoordinator起動要求
└─ cli/                    # CLI Clientとheadless Coordinator起動要求
```

依存方向:

```text
desktop ─┐
         ├→ control-protocol ← runtime(ProjectCoordinator) → application → domain
cli ─────┘                         │                           │
                                  └→ scheduler → domain       │
                                                              │
desktop / cli → bootstrap → runtime                            │
                         ├→ project-documents ─────────────────┤
                         ├→ event-journal ─────────────────────┤
                         ├→ codex-adapter ← codex-protocol ────┤
                         ├→ local-check-adapter ────────────────┤
                         ├→ git-adapter ────────────────────────┤
                         ├→ github-adapter ─────────────────────┤
                         ├→ conflict-analyzer ──────────────────┤
                         ├→ build-adapter ───────────────────────┤
                         └→ persistence ────────────────────────┘
```

図中のAdapterはApplicationが所有するPortを実装するため、crate依存としては`adapter → application → domain`となる。Applicationから具体Adapterへ依存してはならない。具体Adapterの選択とDIは`bootstrap`へ一元化し、DesktopとCLIはlease取得に成功した場合だけ共通bootstrapを呼び出す。Clientとして既存Coordinatorへ接続する場合、Adapterを初期化しない。

`scheduler`はDomain型を入力として利用できるが、Application、Adapter、Tokioへ依存しない。実行資源を扱うProjectCoordinator、Supervisor、Tokio workerは`runtime`へ置き、純粋な`plan`関数と分離する。ApplicationのPortとUse CaseもTokio固有型へ依存せず、`runtime`が非同期実行方式を提供する。

### 禁止する依存

- `domain → application/adapters/UI`
- `application → concrete adapter`
- `desktop component → SQLite/Git/Codex/GitHub`
- `desktop/cli → concrete adapter`によるbootstrapを経由しない直接操作
- `adapter A → adapter B`による隠れた処理連鎖
- wire DTOやDB rowをDomain Entityとしてそのまま利用すること
- GUIとCLIで状態遷移ロジックを別実装すること

依存ルールは`cargo metadata`等を使ったアーキテクチャテストで継続的に検査する。

### 外部依存の許可表

外部技術への直接依存を次のcrateへ限定する。表にないcrateからの利用は禁止する。

| 外部技術・固有型 | 直接依存を許可する場所 | 外へ公開する型 |
| --- | --- | --- |
| Dioxus | `desktop` | Application Command、View DTOのみ |
| Tokio | `runtime`、I/O Adapter、`bootstrap` | Domain/ApplicationへTokio型を公開しない |
| Markdown/YAML parser | `project-documents` | 検証済みDocument DTO、Domain ID |
| SQLite driver / SQL row | `persistence` | Query DTO、Projection更新結果 |
| Codex JSON-RPC wire型 | `codex-protocol`、`codex-adapter` | `AgentRuntime`の内部DTOとEvent |
| Local Check process / output | `local-check-adapter` | `LocalVerification`の要求・完了DTO |
| Git executable/output | `git-adapter` | `GitWorkspace`の内部DTOと正規化error |
| GitHub API/HTTP client型 | `github-adapter` | Verification DTOとEvidence |
| `syn` AST型 | `conflict-analyzer`のparser module | `SymbolRecord`、`SymbolDelta` |
| Bevy | ゲームproject、任意の`game-integration` | Build metadataとprocess結果のみ |

Bevyはコントロールプレーンのworkspaceへcompile-time dependencyとして追加しない。Git CLI、Codex App Server、GitHub Actionsは専用Adapter以外から起動・通信しない。

CIのアーキテクチャテストでは少なくとも次を検査する。

- `cargo metadata`から禁止されたcrate間依存がないこと。
- `domain`、`application`、`scheduler`がDioxus、Tokio、SQLite driver、HTTP client、`syn`へ依存していないこと。
- 外部protocol crateをFacade以外が直接参照していないこと。
- Adapter crateが外部固有型をpublic APIから再公開していないこと。
- Bevyがコントロールプレーンのdependency graphへ入っていないこと。
