# GUI設計

> Status: Draft / MVPのGUI設計基準  
> Scope: デスクトップGUIの情報設計、画面構成、操作、状態表現、視覚・アクセシビリティ要件  
> Domain source: [`domain-model.md`](domain-model.md)  
> Product source: [`MVP.md`](MVP.md)

## 1. この文書の目的

本書は、制作意図、AI実行、技術的Evidence、人間の判断を一つのデスクトップGUIで安全に進めるための設計を定義する。

GUIの中心は、会話、コード、ログではなく次の三つの変換である。

```text
Intent to Work
  制作意図を、承認済みTask Contractと実行計画へ変換する

Work to Evidence
  Task Runの成果を、追跡可能な統合commitと技術的Evidenceへ変換する

Evidence to Decision
  検証済みBuildを、人間の受け入れ判断とmainへのmergeへ変換する
```

本書は画面上の状態名や操作がDomain Modelと異なる意味を持つことを禁止する。Domainの不変条件は[`domain-model.md`](domain-model.md)を正とし、GUIはそれを説明し操作する面として設計する。

## 2. UX原則

### 2.1 人間が判断すべきことを前面に出す

- AIの全会話や全イベントを巡回させない。
- 現在ブロックしている判断、失敗、失効、競合を優先して表示する。
- 自動化可能な待機や再試行を、人間の要対応項目と混在させない。
- AI推奨は根拠と影響を伴う提案として表示し、人間の判断と同じ見た目にしない。

### 2.2 状態ではなく意味を伝える

- `STALE`だけでなく「Candidateのcommitが変わったため、Build 42の受け入れは現在版へ適用できません」と表示する。
- 操作できないボタンを理由なしにdisabledにしない。
- 失敗、外部サービス停止、結果不明、依存待ちを一つの「エラー」にまとめない。
- 状態は保存された事実から導出し、GUI独自の推測で進捗率を作らない。

### 2.3 同一commitの連鎖を常に確認できるようにする

Candidate、CI、AI Review、Build、Acceptance Result、PR headのcommitが一致しているかを、Build & Acceptance画面の最重要情報として扱う。

- commitは先頭8文字を表示し、hoverまたは展開でfull SHAを示す。
- 色だけで一致を表さず、`一致`、`不一致`、`未確認`を文字とiconで示す。
- commit変更時は過去の成功表示を消さず、`過去版`または`現在版には無効`と明記する。

### 2.4 段階的に詳細を開く

情報は三段階で提示する。

1. Overview: 件数、主要状態、次に必要な行動
2. Detail: Task Contract、Evidence、影響範囲、状態遷移
3. Deep inspection: ZedまたはGitHubでコード、diff、生ログを確認

本システム内へコードエディタ、汎用terminal、完全なCIログviewer、Codex全文chatを再実装しない。

### 2.5 危険な操作は結果を先に見せる

Contract変更、実行中Runのcancel、Candidate revision作成、受け入れ、mergeでは、実行前に次を表示する。

- 操作対象
- 現在のrevisionとcommit
- 影響を受けるTask、Run、Build、Acceptance
- 無効になるEvidence
- 成功後に起きる状態遷移
- 失敗または結果不明時の扱い

## 3. 利用者と主要ジョブ

MVPの主利用者は、一人または少人数でゲームを制作する人間の責任者である。日常的に次を行う。

| Job | GUIが支援すること | 外部ツールへ委ねること |
| --- | --- | --- |
| 制作意図を整える | Vision、原則、仮説、受け入れ基準の関連を表示・編集する | 大規模なMarkdown編集はZedでも可能 |
| AIの計画を承認する | Decision、Task DAG、scope、並列計画を比較する | なし |
| 並列作業を監督する | Queue、Run状態、依存、競合、品質ゲートを集約する | コード・diff詳細はZed |
| 技術的失敗を判断する | 失敗要約、関連Task/基準、次の選択肢を示す | 生ログとcheck詳細はGitHub |
| ゲームを受け入れる | Build起動、scenario、Observation、判定、merge gateを一続きにする | ゲーム実行自体はBevy process |
| 例外を処理する | Human Inboxへ理由、推奨、影響、選択肢を集約する | 必要ならZed/GitHubで追加調査 |

## 4. 情報アーキテクチャ

### 4.1 画面構成

単一ウィンドウ内に、五つの主画面と二つの詳細画面を持つ。

```text
Project Intent ─→ Plan Review ─→ Development Board ─→ Build & Acceptance
                                      │                         │
                                      ├─ Task Run Detail        └─ Candidate / Build Detail
                                      │
                                      └────────→ Human Inbox ←──┘
```

| 主画面 | 役割 | 主なEntity |
| --- | --- | --- |
| Project Intent | 何を作り、何を確かめたいかを管理する | Vision、Principle、Hypothesis、Backlog、Acceptance Criterion |
| Plan Review | AI提案を実行可能な計画として承認する | Decision、Task、Task Contract、Task Dependency |
| Development Board | 実行・統合・品質ゲートを監督する | Task、Task Run、Integration Candidate、Evidence |
| Human Inbox | 人間にしか解けない判断を処理する | Human Inbox Item、Decision Request、影響対象 |
| Build & Acceptance | Buildをプレイし、受け入れ、mergeする | Candidate Revision、Build、Playtest Session、Acceptance Result |

Task Run DetailとCandidate Detailは独立した大型アプリにせず、Development Boardから開く副routeまたは詳細ペインとする。

### 4.2 ナビゲーション

主navigationは三つの変換を保ちながら、日常操作する五画面へ直接移動できる構造にする。

```text
┌─────────────────────────────────────────────────────────────┐
│ Project: Powder Game       Coordinator ● Connected          │
├──────────────┬──────────────────────────────────────────────┤
│ INTENT       │                                              │
│  Intent      │                                              │
│  Plan Review │               Main Content                   │
│              │                                              │
│ WORK         │                                              │
│  Development │                                              │
│              │                                              │
│ DECIDE       │                                              │
│  Inbox   3   │                                              │
│  Build       │                                              │
├──────────────┴──────────────────────────────────────────────┤
│ Sync r1842 · main 91ad40c1 · 2 running · 1 needs attention │
└─────────────────────────────────────────────────────────────┘
```

- 左sidebarは常時表示を基本とし、狭い幅ではicon railへ縮小する。
- Human Inboxには未処理件数をbadge表示する。単なる通知総数ではなく、人間の対応が必要な件数だけを数える。
- 現在地は色だけでなく背景、左border、`aria-current`で示す。
- Entity間の移動では`TASK-12 / RUN-12-3 / CAND-4 rev 2 / BUILD-42`のbreadcrumbを表示する。
- 戻る操作でfilter、scroll位置、選択中Entityを復元する。

### 4.3 Route案

```text
/intent
/plan
/development
/development/tasks/:task_id
/development/runs/:task_run_id
/development/candidates/:candidate_id
/inbox
/inbox/:item_id
/acceptance
/acceptance/builds/:build_id
```

Dioxusの内部route名は変えてよいが、deep link可能な安定したEntity IDをrouteへ含める。

## 5. Application Shell

### 5.1 Top bar

常に次を表示する。

- project名とroot
- 現在の`main` commit
- Coordinator接続状態
- Read Modelの同期状態
- global command/search入口
- settings入口

Coordinatorへ接続できない場合は、Clientがメモリ上に保持している直近のView snapshotをread-onlyで表示できる。ただし画面上部へ持続bannerを出し、すべてのMutation操作を理由付きで停止する。ClientがSQLiteを直接読んでfallbackしてはならない。

### 5.2 Bottom status bar

詳細画面へ移動しなくても、次を把握できるようにする。

- Projection revisionと最終更新時刻
- 実行中Run数 / concurrency上限
- Queue数
- 人間待ち件数
- GitHub、Codex、Build runnerの接続異常

正常時に大量のgreen iconを並べず、異常と待機を優先する。

### 5.3 Global command palette

`Ctrl/Cmd + K`で次を検索・実行できる。

- Task、Task Run、Candidate、Build、Inbox ItemをIDまたはtitleで開く
- Development Boardのfilterを適用する
- Zedで現在対象を開く
- Build & Acceptanceへ移動する
- read model再同期などの安全な診断操作

受け入れ、Contract承認、mergeなどの重大Commandはcommand paletteから即時実行せず、対象画面の確認stepへ遷移する。

## 6. Project Intent

### 6.1 目的

遊びの核からAcceptance Criterionまでの制作意図を、人間が読める文書と追跡可能な関連として管理する。MVPでは高機能な文書editorを目指さず、Markdownの構造化fieldと本文の編集、検証、Zed連携に絞る。

### 6.2 Layout

```text
┌──────────────────────────────────────────────────────────────┐
│ Project Intent                         [Validate] [Open Zed] │
├───────────────┬────────────────────────────┬─────────────────┤
│ Documents     │ Editor / Preview           │ Traceability    │
│               │                            │                 │
│ Vision        │ 遊びの核                   │ Hypotheses  3   │
│ Principles    │ ...                        │ Backlog     8   │
│ Hypotheses    │                            │ Criteria   12   │
│ Milestones    │                            │                 │
│ Backlog       │                            │ Validation ✓    │
│ Acceptance    │                            │                 │
└───────────────┴────────────────────────────┴─────────────────┘
```

### 6.3 必須表示・操作

- 文書種別、ID、title、status、schema version、最終更新
- 本文と構造化fieldの編集
- ID参照の存在、schema、重複、循環の検証結果
- Hypothesis → Backlog → Acceptance Criterion → Taskの関連
- `保存`、`変更を破棄`、`Zedで開く`、`計画を生成`または`Plan Reviewへ`

外部editorによる同時変更を検出した場合、last-write-winsで保存しない。自分の未保存変更、disk上の変更、共通baseの存在を示し、`再読込`、`Zedで比較`、`Human Inboxへ送る`を選ばせる。

### 6.4 Empty / validation states

- 初期projectでは、Vision作成から始める一つのprimary actionだけを示す。
- schema errorはfieldの近くと画面上部のsummaryの両方へ表示する。
- AIへ計画生成を依頼できない場合、欠けているVision、Acceptance Criterion等を具体的に列挙する。

## 7. Plan Review

### 7.1 目的

AIが提案した仕様、設計、Task分解、依存DAG、並列計画を、人間が影響を理解して承認する。

### 7.2 Layout

```text
┌───────────────────────────────────────────────────────────────┐
│ Plan Review · proposal 7          2 decisions · 1 DAG issue  │
├──────────────────────┬────────────────────────────────────────┤
│ Review checklist     │ Proposal                               │
│ ○ Design decisions   │ [Summary] [Tasks] [Dependencies]      │
│ ○ Acceptance mapping │                                        │
│ ○ Task scope         │ TASK-12  Grid API                      │
│ ○ Dependencies       │ scope · tests · risk · dependencies    │
│ ○ Parallel plan      │                                        │
├──────────────────────┴────────────────────────────────────────┤
│ Impact: 8 tasks / critical path 4 / max parallel 2           │
│                                  [Request changes] [Approve]  │
└───────────────────────────────────────────────────────────────┘
```

### 7.3 Review単位

| Tab | 表示内容 | 確認する不変条件 |
| --- | --- | --- |
| Summary | 遊びの核との対応、非目標、主要リスク | 提案が意図から逸脱していない |
| Decisions | 選択肢、AI推奨、根拠、影響Task | 未解決の必須Decisionがない |
| Tasks | Task Contract、scope、test plan、risk | Contractが実行可能である |
| Dependencies | DAG、依存種別、理由、critical path | 参照先存在、自己依存なし、循環なし |
| Parallel Plan | 実行wave、競合予測、上限 | `blocks_start`と競合制約を守る |

DAGは視覚図だけに依存しない。keyboard操作可能な依存listと、`TASK-12はTASK-03の受け入れ待ち`の文章表現を併設する。

### 7.4 承認操作

Plan全体の承認前に、未解決Decision、invalid Contract、DAG errorが0であることを確認する。承認dialogには次を示す。

- 作成または更新されるTask文書
- 有効化される依存制約
- 実行可能になるTask数
- 既存Runが`STALE`になる場合の件数と理由

単なる`OK / Cancel`ではなく、`この計画を承認`と`戻って修正`を使用する。

## 8. Development Board

### 8.1 目的

複数Codexの会話一覧ではなく、Task、Task Run、Integration Candidate、品質ゲートの現在地と、人間が次に行うべきことを一画面で把握する。

### 8.2 Overview

```text
┌────────────────────────────────────────────────────────────────────┐
│ Development  Running 2/2 · Queue 3 · Blocked 2 · Attention 1      │
│ [All] [Runnable] [Running] [Failed] [Human wait]     Search / Sort │
├─────────┬──────────────────┬──────────────┬───────┬──────┬────────┤
│ Task    │ Current Run      │ State        │ Local │ Risk │ Target │
├─────────┼──────────────────┼──────────────┼───────┼──────┼────────┤
│ TASK-01 │ RUN-01-2        │ Running      │ 2/4   │ Safe │ CAND-4 │
│ TASK-02 │ RUN-02-1        │ CI failed    │ Pass  │ Low  │ CAND-4 │
│ TASK-03 │ —               │ Dependency   │ —     │ —    │ —      │
└─────────┴──────────────────┴──────────────┴───────┴──────┴────────┘
│ Candidate CAND-4 rev 2 · 4 runs · commit 91ad40c1 · CI running    │
└────────────────────────────────────────────────────────────────────┘
```

Kanbanだけでは、Taskの論理状態、Runの実行段階、直交するHealth/Flag、複数品質ゲートを比較しにくい。そのためMVPではtableをprimary viewとし、必要なら実行段階別のgroup viewをsecondary viewとして追加する。

### 8.3 Summary cards

上部には次だけを表示する。

- Running / concurrency上限
- Runnable
- Queued
- Dependency blocked
- Human action required
- Failedまたはresult unknown

タスク総数から推定した進捗percentageを主指標にしない。Backlogの価値到達状態は別のprogress viewとして扱う。

### 8.4 Task row

一行で次を比較できるようにする。

- Task IDとtitle
- 採用候補または最新のTask Run
- 主状態
- `STALE`、`SCOPE_VIOLATION`、`CONFLICT_RISK`等の独立badge
- Local/TDD/scope gateのsummary
- dependency block理由
- Candidateへの採用状況
- 人間が必要な次のaction

Taskに複数Runがある場合、失敗Runを隠して最新Runだけを正解扱いしない。row展開で全Runを時系列表示し、CandidateがどのRunを採用しているかを明示する。

### 8.5 Task Run Detail

Task row選択時は右detail paneを開き、より広い調査が必要な場合だけfull detail routeへ遷移する。

```text
RUN-02-3 · TASK-02 砂の落下                         [Open in Zed]
SUCCEEDED   SCOPE OK   CONFLICT RISK

Contract rev 4    Base 3af271c0    Output 80c10b6a

[Overview] [TDD] [Changes] [Evidence] [Events]

Lifecycle
Queued 10:02 → Agent 10:04 → Local checks 10:19 → Succeeded 10:23

Next action
同一symbolをRUN-03-1も変更しています。統合順序を確認してください。
                                                     [Review conflict]
```

必須情報:

- Task目的、非目標、Acceptance Criterion
- Contract revision、base commit、output revision
- dependencies、allowed/forbidden paths
- worktree、branch、Codex threadの識別情報
- TDD cycleとRed/Green/final suite
- 変更fileとscope check
- local check、CI、AI Review、Conflict Finding
- Decision/Input request
- event timelineとoperation certainty

生ログを埋め込まず、要約、digest、最後の数行、`GitHubで開く`または`Zedで開く`を表示する。

### 8.6 Candidate composer / detail

Candidate detailでは、Task Runを単にcheckboxでまとめない。依存と競合から提案された順序、採用Runのrevision、統合後commitを示す。

```text
CAND-4 · revision 2 · COMPOSING

Integration order
1. RUN-01-2  Grid API       reason: blocks_integration
2. RUN-02-3  Sand behavior  reason: depends on Grid API
3. RUN-05-1  Input          reason: independent

Conflict analysis
✓ 2 safe pairs   ! 1 potential conflict   ? 0 unknown

[Re-analyze] [Create revision and compose]
```

- 同じTaskのRunを複数選択したら、その場で拒否理由を表示する。
- `STALE`、scope違反、TDD違反のRunは選択不可にし、修正への導線を示す。
- Candidate revisionを作る前に、旧Build/Evidenceが無効になる場合は影響previewを表示する。
- Candidateのcommit、revision、PR headが一致しない場合は最上部へpersistent warningを表示する。

### 8.7 Boardの総合状態優先順位

総合状態はrowをscanするためのProjectionであり、元の状態を隠さない。優先順位は次とする。

```text
Human action required
  > Failed / Result unknown
  > Scope or TDD violation
  > Stale / commit mismatch
  > Conflict blocked
  > Running
  > Queued / Dependency wait
  > Succeeded / Integrated / Accepted
```

たとえば`SUCCEEDED + SCOPE_VIOLATION`は「Succeeded」ではなく「Scope violation」を総合表示し、detailには両方を残す。

## 9. Human Inbox

### 9.1 目的

設計判断、承認要求、高リスク競合、重大review finding、外部変更競合、プレイ判断など、人間の判断が必要な項目を一か所へ集約する。

### 9.2 Layout

```text
┌────────────────────────────────────────────────────────────────┐
│ Human Inbox   Open 3 · Blocking 2                              │
├──────────────────────┬─────────────────────────────────────────┤
│ [High] Trait変更     │ Material traitを変更するか              │
│ Blocks 3 tasks       │                                         │
│                      │ Why now                                 │
│ [Med] Scope request  │ 火と水で温度を扱う必要が生じた          │
│ Blocks RUN-08        │                                         │
│                      │ Options                                 │
│ [Low] Input request  │ ○ Componentへ分離  AI recommended       │
│                      │ ○ Traitへ追加                           │
│                      │ ○ 今回は見送る                          │
│                      │                                         │
│                      │ Impact: TASK-12, 13, 16                 │
│                      │ [Ask research] [Defer] [Choose option]  │
└──────────────────────┴─────────────────────────────────────────┘
```

### 9.3 Listの並び順

既定順は次の安定したkeyを使う。

1. `blocking = true`
2. severity
3. blocked critical-path impact
4. created_at
5. item ID

AIが推測した緊急度だけで自動並べ替えしない。filterはOpen/Deferred/Resolved、severity、subject type、blocked Task、作成者を提供する。

### 9.4 Decision detail

必ず次を表示する。

- 何を決めるのか
- なぜ今必要か
- 選択肢と各trade-off
- AI推奨と、その根拠
- 影響するTask、Run、Candidate、Build
- 現在ブロック中の処理
- 回答後に実行されるCommandと状態遷移
- 追加調査、保留、手動対応への切替

回答には理由を必須とする。AI推奨optionを視覚的に示してよいが、既定選択済みにしてはならない。

### 9.5 Inbox itemの完了

回答送信後すぐに一覧から消さず、`反映中`として表示する。関連Commandの成功を観測した後に`Resolved`へ移す。外部操作がresult unknownの場合はOpenへ戻さず、`結果確認中`としてReconciliationへの導線を示す。

## 10. Build & Acceptance

### 10.1 目的

技術検証済みのCandidate commitからBuildを生成し、人間が同じcommitをプレイし、Acceptance Decisionを記録し、merge gateを確認して`main`へ進める。

### 10.2 Layout

```text
┌───────────────────────────────────────────────────────────────────┐
│ Build & Acceptance · CAND-4 rev 2 · commit 91ad40c1              │
├─────────────────┬─────────────────────────────────────────────────┤
│ Builds          │ BUILD-42 · READY · Linux                       │
│                 │ artifact 6d8e…                                 │
│ ● BUILD-42      │                                                 │
│ ○ BUILD-41 old  │ Included: TASK-01, TASK-02, TASK-05             │
│                 │ Technical gate                                 │
│                 │ ✓ Local  ✓ CI  ✓ AI Review  ✓ Commit match     │
│                 │                                                 │
│                 │ Playtest scenarios                             │
│                 │ 1. 砂を直感的に配置できるか                    │
│                 │ 2. 落下挙動を理解できるか                      │
│                 │                                                 │
│                 │ [Launch game]                                  │
├─────────────────┴─────────────────────────────────────────────────┤
│ Acceptance: Not recorded                         [Start session] │
└───────────────────────────────────────────────────────────────────┘
```

### 10.3 Build registry

各Buildに次を表示する。

- Build ID、status、platform
- Candidate ID、revision、source commit
- 含有TaskとTask Run snapshot
- artifact URI/hash
- CI/AI Reviewのsummaryと残余リスク
- Playtest SessionとAcceptance Decision
- `Current`、`Past revision`、`Superseded`の区別

古いBuildを一覧から消さない。既定ではCurrentだけを上にまとめ、Past revisionを折り畳む。

### 10.4 Playtest flow

```text
Build READY
  → scenario確認
  → Start session
  → Launch game
  → game終了を観測
  → Observation入力
  → Acceptance Decision
  → Candidate状態とmerge gateを再評価
```

ゲーム起動前にBuild ID、commit、今回追加されたTask、確認scenarioを表示する。起動失敗とゲーム内の不具合を区別する。game processがcrashしても自動的に`DEFECT`判定を作らず、Observation候補として人間へ提示する。

### 10.5 Acceptance form

Acceptance Resultでは、判定とObservationを分離する。

```text
今回のBuildを受け入れますか？

( ) 受け入れる
    現在のcommitをmainへ進められます。

( ) 変更が必要
    Candidateを改訂し、品質ゲートとプレイをやり直します。

Observations
[Defect] [Enhancement] [Manual tuning] [Other]
内容、再現手順、関連scenario、添付参照

Follow-up
[ ] Observationから新しいTaskを作る

理由（必須）
...
```

UI上の代表操作はDomainへ次のように対応させる。

| UI操作 | Acceptance Decision | Observation / 後続処理 |
| --- | --- | --- |
| 受け入れる | `ACCEPTED` | 必要ならEnhancementを新規Task化 |
| AIへ修正依頼 | `CHANGES_REQUIRED` | 原則`DEFECT`、新しいTask Runを作成 |
| 改善を新規Task化して今回は受け入れる | `ACCEPTED` | `ENHANCEMENT`と後続Taskを記録 |
| 改善してから受け入れる | `CHANGES_REQUIRED` | `ENHANCEMENT`と修正Task/Runを記録 |
| 手動で微調整 | `CHANGES_REQUIRED` | `MANUAL_TUNING`、Zedで開き再検証 |

### 10.6 Commit mismatch / stale表示

受け入れ後にCandidate commitが変わった場合、過去Resultを赤い失敗として扱わない。履歴上は成功した人間判断であり、現在版への適用だけが失効している。

```text
┌─ Previous acceptance no longer applies ─────────────────────┐
│ BUILD-42 was accepted at commit 91ad40c1.                   │
│ Current CAND-4 rev 3 is commit 3bc771a8.                    │
│ CI, AI Review, Build, and Playtest must run again.          │
│                                      [View changes] [Re-run] │
└──────────────────────────────────────────────────────────────┘
```

表示規則:

- 過去Resultへ`Accepted · past revision` badgeを付ける。
- 現在Candidateのheaderは`Not accepted`へ戻す。
- 旧Resultをmerge gateへ使用しない。
- どの操作でrevisionが変わったかをtimelineへ表示する。

### 10.7 Merge gate

merge buttonの直前に、同一commitの連鎖をchecklistで示す。

```text
Merge readiness
✓ Candidate current revision: rev 3
✓ Candidate commit:          3bc771a8
✓ PR head:                   3bc771a8
✓ Required CI:               PASS @ 3bc771a8
✓ AI review:                 PASS @ 3bc771a8
✓ Build:                     BUILD-45 @ 3bc771a8
✓ Acceptance:                RESULT-18 ACCEPTED @ 3bc771a8
✓ Governing result:          RESULT-18
✓ Blocking health flags:     none
✓ main base compatible

[Merge accepted candidate]
```

一つでも`FAIL`、`STALE`、`UNKNOWN`、`PENDING`ならbuttonをdisabledにし、項目内へ解決actionを表示する。merge確認dialogではsource commitと、merge成功後に受け入れとなるTaskを表示する。

merge要求後は`MERGING`を表示し、二重送信を禁止する。timeout時に失敗と断定せず`結果確認中`へ遷移し、GitHub状態をReconcileするまで再mergeを許可しない。

## 11. 状態表現

### 11.1 Status chip

すべてのStatus Chipはicon、短いlabel、semantic colorを持つ。色だけで状態を伝えない。

| 意味 | 表示例 | 色token | icon例 |
| --- | --- | --- | --- |
| 成功・有効 | Passed、Ready、Accepted | `success` | check |
| 実行中 | Running、Building、Merging | `info` | spinner/progress |
| 待機 | Queued、Dependency wait | `neutral` | clock/pause |
| 人間待ち | Decision required | `attention` | person/message |
| 警告 | Conflict risk、Past revision | `warning` | triangle |
| 失効 | Stale、Superseded | `stale` | history/refresh |
| 失敗 | Failed、Scope violation | `danger` | x/octagon |
| 不明 | Unknown、Result unknown | `unknown` | question |

`Passed`と`Accepted`を同じ文言にせず、どの段階の成功かを必ず示す。

### 11.2 Gate state

品質ゲートは次の共通状態を使う。

```text
NOT_REQUIRED | NOT_STARTED | RUNNING | PASSED | FAILED | STALE | UNKNOWN
```

- `NOT_REQUIRED`と`NOT_STARTED`を同じdash記号で表さない。
- `FAILED`には検査不合格、`UNKNOWN`には通信切断や結果未確定を割り当てる。
- `STALE`は過去に結果が存在したが、現在のcommitには適用できない状態とする。

### 11.3 Entity identity

Task、Run、Candidate、Build、Resultをtitleだけで表示しない。詳細header、dialog、toast、deep linkには必ず型付きIDを含める。commitやhashはmonospaceで表示し、copy actionを用意する。

### 11.4 時刻と鮮度

- 相対時刻と絶対時刻を併記可能にする。例: `3分前（2026-08-01 12:30 JST）`。
- 外部状態には`last observed`を表示する。
- 古いProjectionを最新状態のように見せない。
- clock差ではなくEvent/Projection revisionを整合性判断に使う。

## 12. Interaction pattern

### 12.1 Command送信

GUIはDomain状態を楽観的に書き換えない。

```text
User Intent
  → confirmation / validation
  → Command送信
  → Submitting表示
  → Command受理
  → Operation/Event進行を購読
  → Projection更新後に確定表示
```

- 二重clickとtimeout再送には同じ`command_id`を使う。
- Aggregate更新には表示時のexpected versionを付ける。
- `VersionConflict`時は入力を失わず、最新内容と差分を表示する。
- 長時間操作は画面をblockingせず、operation IDで追跡する。

### 12.2 Confirmation level

| Level | 操作例 | Pattern |
| --- | --- | --- |
| Low | filter、詳細を開く、再読込 | 即時、undo不要 |
| Medium | Queue投入、CI再実行、Build要求 | 影響summary付きaction |
| High | Contract変更、active Run cancel、Candidate改訂 | impact preview + 理由 |
| Critical | Acceptance、main merge | commit/gate再照合 + 明示的確認 |

確認dialogを連発しない。不可逆性やEvidence失効がある操作に限定する。

### 12.3 Toastと永続message

- 一時的な成功はtoastでよい。
- 人間対応が必要な失敗、commit mismatch、Coordinator切断は消えるtoastだけにしない。
- `Command accepted`と`Operation completed`を区別する。
- toastから関連Entityまたはoperation detailへ移動できるようにする。

### 12.4 Cancel

実行中Task Runのcancelは「停止要求」と「停止観測」を分ける。click直後に`CANCELLED`へせず、`Cancellation requested`を表示する。未記録変更があるworktreeを自動削除しない。

## 13. Loading、Empty、Error、Recovery

| 状態 | 表示方針 |
| --- | --- |
| Initial loading | layoutを維持したskeletonと、読込対象を示す |
| Background refresh | 既存内容を維持し、小さな同期indicatorを出す |
| Empty | なぜ空かと、開始できる一つのprimary actionを示す |
| Domain rejected | 破った不変条件と修正可能な入力を示す |
| Policy deferred | 依存・資源・人間待ちの理由と再評価条件を示す |
| External unavailable | 対象system、last observed、retry可否を示す |
| Result unknown | 成功/失敗を決めつけずReconciliation状態を示す |
| Projection stale | read-only表示と最終revision、再同期actionを示す |
| Version conflict | 最新revision、競合field、自分の入力を並べる |

内部error codeをそのままprimary messageにせず、診断detailとしてcopy可能にする。

## 14. 視覚設計

### 14.1 Direction

AI製品らしい装飾より、制作コントロールプレーンとしての判読性と信頼感を優先する。黒鉛、鍛造金属、精密工具を基調としたlight / night themeを持ち、情報密度の高いtable、明瞭な階層、控えめなmotionを用いる。

### 14.2 Color tokens（dark theme案）

| Token | 値 | 用途 |
| --- | --- | --- |
| `bg.canvas` | `#0F141B` | window背景 |
| `bg.surface` | `#171E28` | panel、sidebar |
| `bg.raised` | `#202A36` | popover、selected row |
| `border.default` | `#344154` | divider、control border |
| `text.primary` | `#F4F7FA` | 主text |
| `text.muted` | `#A8B3C1` | 補助text |
| `accent` | `#70B7FF` | focus、primary action |
| `success` | `#55C28A` | valid success |
| `warning` | `#E8B44F` | risk、past revision |
| `danger` | `#F07178` | failure、violation |
| `attention` | `#C39BF3` | human decision required |
| `stale` | `#9B8AC4` | stale、superseded |
| `unknown` | `#B5BEC9` | result unknown |

実装時にはWCAG contrastを測定し、通常textは4.5:1以上、大きなtextとUI境界は3:1以上を満たすよう調整する。semantic colorは文字、icon、形状と組み合わせる。

Light / Nightの切替はlocal UI stateとして扱い、TaskやProjectのDomain状態へ保存しない。どちらのthemeでも情報階層、semantic colorの意味、操作可能性を変えてはならない。

### 14.3 Typography

- UI: OS native sans-serifを優先する。
- ID、commit、path、command: monospace。
- 既定本文14px、補助12px、画面title20px、section title16pxを基準にする。
- 日本語のline-heightは本文1.55以上、table cellは1.4以上を確保する。
- ALL CAPSを長い日本語labelへ使用しない。Domain enumはcode表示のときだけ大文字を保つ。

### 14.4 UI language

- navigation、操作、system status、Domain enumは英語へ統一する。
- projectで人間が定義したtitleや本文と、長い補足説明は日本語を許容する。
- 同じ階層の概念を英語と日本語で混在させない。たとえば`HUMAN WAIT`はTask Run状態、`NEEDS REVIEW`は人間の確認件数として区別する。
- 将来のlocale切替までは、同じcontrol内へ英語と日本語を併記しない。

### 14.5 Spacing and density

- 4px基準のspacing scaleを使う。
- 通常row高40px、compact table 32px、主要button 36px以上を基準にする。
- statusを詰め込みすぎず、Overviewでは3個を超えるbadgeを`+N`へまとめ、展開で全件表示する。
- border radiusは6px前後とし、cardの多重nestingを避ける。

### 14.6 Motion

- 状態遷移の強調は150–200msのopacity/background transitionに留める。
- 実行中spinner以外の常時animationを避ける。
- `prefers-reduced-motion`ではtransitionと自動scrollを抑制する。
- 重要な状態変更はanimationだけで伝えない。

## 15. AccessibilityとKeyboard

- すべての操作をkeyboardで到達・実行できる。
- focus indicatorを常に視認可能にし、dialogを閉じたら起点へfocusを戻す。
- tableはcolumn header、row label、sort状態をscreen readerへ伝える。
- status、severity、commit一致を色だけで区別しない。
- DAG、timeline、graphには同内容のlist/table表現を用意する。
- live updateは`aria-live`を乱用せず、人間対応が必要な変更だけを通知する。
- error summaryから各fieldへ移動できる。
- icon-only buttonにはaccessible nameとtooltipを付ける。

主要shortcut案:

| Shortcut | 操作 |
| --- | --- |
| `Ctrl/Cmd + K` | Command palette |
| `g`, `d` | Development Boardへ移動 |
| `g`, `i` | Human Inboxへ移動 |
| `g`, `b` | Build & Acceptanceへ移動 |
| `/` | 現在画面の検索へfocus |
| `j` / `k` | listの次/前項目 |
| `Enter` | 選択項目のdetailを開く |
| `Esc` | dialog/detail paneを閉じる |

文字入力中はsingle-key shortcutを無効にする。

## 16. Window sizeとresponsive behavior

desktopを主対象とし、mobile専用UIはMVPに含めない。

| 幅 | Layout |
| --- | --- |
| 1440px以上 | sidebar + main + persistent detail pane |
| 1100–1439px | sidebar + main、detailはoverlay drawer |
| 900–1099px | icon rail + single main column、table列を優先度順に省略 |
| 900px未満 | 最小対応外。利用可能だが主要操作時にwindow拡大を案内 |

最低window sizeは1024×720を実装目標とする。幅が狭くてもcommit mismatch、Human Inbox、merge gateなどの安全情報を非表示にしてはならない。

## 17. GUIとApplication層の境界

GUI componentはGit、Codex、GitHub、SQLiteを直接操作しない。Read Model Queryを描画し、User Intentを型付きCommandへ変換する。

### 17.1 View DTO

画面専用のDTO例:

```text
DevelopmentBoardView {
  projection_revision,
  summary,
  task_rows[],
  current_candidate_summary,
  filters,
  freshness
}

BuildAcceptanceView {
  candidate_ref,
  build_rows[],
  selected_build,
  technical_gate,
  playtest_session,
  governing_acceptance,
  merge_readiness,
  freshness
}
```

DB row、GitHub SDK型、Codex wire DTO、Dioxus signalをView DTOへ露出させない。

### 17.2 User IntentとCommand

```text
UI Intent                         Application Command
Queue selected task       →      QueueTaskRun
Stop run                 →      CancelTaskRun
Approve plan             →      ApprovePlan
Choose decision option   →      AnswerDecisionRequest
Compose candidate        →      ComposeIntegrationCandidate
Request build            →      RequestBuild
Launch / finish playtest →      StartPlaytest / CompletePlaytest
Submit acceptance        →      RecordAcceptanceResult
Merge                    →      MergeAcceptedCandidate
```

Componentが次状態を直接setしてはならない。Commandの受理、Operation結果、Event、Projection更新を通して表示を更新する。

## 18. Component inventory

MVPで共通化するcomponentを次に限定する。

| Component | 用途 |
| --- | --- |
| `EntityHeader` | 型付きID、title、状態、revision、commit |
| `StatusChip` | 状態、Health/Flag、Gate state |
| `EntityLink` | Task/Run/Candidate/Build/Result間の遷移 |
| `GateChecklist` | local、CI、review、build、acceptance、merge条件 |
| `EvidenceSummary` | 対象commit、結果、鮮度、外部link |
| `ImpactPreview` | 無効化・block・状態遷移の事前表示 |
| `DecisionCard` | 選択肢、推奨、根拠、影響、回答 |
| `OperationProgress` | requested/running/result unknown/completed |
| `EventTimeline` | 状態遷移とactor、理由、correlation |
| `DetailPane` | list選択の文脈を保つ詳細表示 |
| `EmptyState` | 空の理由と一つのprimary action |
| `PersistentBanner` | 切断、stale、commit mismatch等の持続警告 |

万能な`Card`へDomain判断を埋め込まず、状態の意味と操作可否は共通selector/View Modelから供給する。

## 19. MVP実装優先順位

### P0: Work to EvidenceからEvidence to Decisionまでを通す

- Application Shellとnavigation
- Development Board table
- Task Run Detail
- Human Inbox list/detail/回答
- Candidate composer/detailと技術gate
- Build registry、game起動、Acceptance form
- merge readinessとcommit mismatch表示
- loading/error/disconnected/read-only状態

### P1: Intent to WorkをGUI内で閉じる

- Project Intentの最小Markdown編集・検証
- Plan Review checklist
- Task Contract detail
- dependency listと簡易DAG
- Plan承認と影響preview

### P2: 理解速度を上げる

- Command palette
- 保存filter/view
- 高度なDAG/critical path visualization
- Evidence比較
- keyboard shortcutの拡充

## 20. MVPでは作らないUI

- 独自コードeditor
- 汎用terminal
- Codex全文chatを主画面にするUI
- GitHub Actionsの完全なlog viewer
- AIが推測した正確そうな進捗percentage
- mobile専用画面
- 高度なゲーム内受け入れHUD
- 自動で人間のAcceptance Decisionを下す操作
- commit mismatchを無視するforce merge

## 21. UIテストと完成条件

### 21.1 Component / reducer test

- Domain状態とHealth/Flagの組合せが正しいStatus Chipと総合状態になる。
- `FAILED`、`STALE`、`UNKNOWN`、`CHANGES_REQUIRED`を成功色で表示しない。
- 同じTaskの複数Runを展開し、Candidate採用Runを識別できる。
- commit mismatch時に旧Acceptanceを`past revision`として残し、現在版を未受け入れと表示する。
- merge readinessの一条件でも不成立ならmergeできない。
- disabled actionが理由と解決導線を持つ。
- Version conflict時に入力を失わない。

### 21.2 Accessibility test

- keyboardだけで五主画面、detail pane、Decision回答、Acceptance入力へ到達できる。
- focus順とdialog focus trapが正しい。
- status、graph、gateがscreen readerで同じ意味を取得できる。
- semantic colorがcontrast基準を満たす。
- 200% zoom相当でもCritical actionと警告が欠落しない。

### 21.3 End-to-end scenario

GUIだけで次を完了できることをMVPのUI完成条件とする。

1. VisionとAcceptance Criterionを確認する。
2. Plan、Task DAG、scopeを承認する。
3. Task RunをQueueし、並列実行状態を追跡する。
4. scope violationまたは競合をHuman Inboxで処理する。
5. Task Run群からCandidate revisionを作成する。
6. 同一commitのCIとAI Reviewを確認する。
7. Buildを生成し、scenarioを見ながらゲームを起動する。
8. ObservationとAcceptance Decisionを記録する。
9. commit一致をmerge readinessで再確認する。
10. Candidateをmergeし、Task受け入れと後続Task解除を確認する。
11. 途中でcommitを変更した場合、旧BuildとAcceptanceが現在版へ適用されないことを確認する。

GUIの完成は、画面が揃うことではなく、この一周を人間が状態の意味と次の影響を理解しながら安全に完了できることで判断する。
