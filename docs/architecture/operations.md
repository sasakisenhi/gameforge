# 運用、監査、設計判断

> [アーキテクチャ概要](../../architecture.md)へ戻る

エラー分類、監査、セキュリティ、ADR候補、アーキテクチャ完成条件を定義する。

## エラー、監査、セキュリティ

### エラー分類

- `DomainRejected`: 不変条件違反、許可されない遷移
- `PolicyDeferred`: 依存待ち、資源待ち、競合不明
- `ExternalUnavailable`: Codex/GitHub/processへの接続失敗
- `ExternalStateConflict`: HEAD不一致、同時編集、PR head変更
- `VerificationFailed`: test、lint、review等の品質不合格
- `InternalFault`: schema不整合、Projection失敗、想定外エラー

これらを単一の`FAILED`表示へ潰さず、再試行、修正、判断、復旧のどれが必要かをUIへ返す。

### 監査

すべての重要CommandとEventにactor、対象、理由、correlation IDを付ける。Gitの破壊的操作、Codexの承認、Task Contract変更、受け入れ、mergeは実行前後の識別情報を残す。

### セキュリティ

- GitHub tokenなどの秘密情報はOSの安全なcredential storeまたは環境からAdapterへ渡し、Markdown、Event、SQLite、ログへ保存しない。
- 外部から受信したpath、URI、branch名、JSON-RPC payloadを境界で検証する。
- repository root外へのpath traversalとsymlinkによるscope逸脱を、実パス解決後に拒否する。
- ログ表示時に秘密情報と個人情報をredactする。

## Architecture Decision Record候補

次の事項は、実例または技術検証後にADRとして確定する。

- Event Journalの物理形式と、Gitへcommitする単位
- `ProjectWriterLease`のOS別lock実装、metadata配置、endpoint発見方式
- `LocalControlChannel`のIPC方式、encoding、認証、protocol互換範囲
- Desktop内包Coordinatorとheadless Coordinatorの既定選択、detachと終了UX
- Domain型とMarkdown schema型をどこまで分離するか
- Application Portで`async_trait`を使うか、明示的なFuture型を使うか
- 単一Writerを維持した上で、SQLite ProjectorをCoordinatorと同一processにするか従属processにするか
- GitHub状態取得をpolling中心にするか、webhookも受けるか
- Codex App ServerをTask Runごとに分離するMVP判断を、長期運用でも維持するか
- Manual AdjustmentをTask Run種別にするか、付随Eventにするか
- Acceptance ResultをScenario単位に分割するか、Session判定を中心にするか
- rust-analyzerを導入する段階と、`syn`解析結果との統合形式
- ローカルArtifact Storeの配置、保持期限、garbage collection方針

ADRでは、選択肢、決定、理由、MVPへの影響、再検討条件を記録する。

## アーキテクチャ完成条件

- Domainの状態遷移と判定を、外部I/Oなしでテストできる。
- GUIとCLIが同じUse Caseと状態遷移を使用する。
- 同じproject rootにDesktopとCLIを同時起動しても、Writer leaseを持つCoordinatorが一つだけである。
- DesktopとCLIが正本、SQLite、Git、Codexを直接変更せず、すべてのMutationがCoordinatorを通る。
- Command再送が`command_id`で重複排除され、同時更新がexpected versionで競合検出される。
- Coordinatorの強制終了後、新しいCoordinatorが未完了OperationとProjectionを安全に復旧できる。
- Git、Codex、GitHub、Build、ZedをAdapterとして交換・fake化できる。
- Schedulerの開始判定が、process管理から独立して再現可能である。
- AST競合解析が不変なrecord集合とruleで説明可能な結果を返す。
- SQLiteを削除しても、MarkdownとEvent Journalから画面状態を再構築できる。
- Build、CI、AI Review、Acceptanceが同じcommitへ結び付く。
- commit不一致、`UNKNOWN`、scope violation、人間判断待ちが安全側に倒れる。
- 再起動後に実行中・結果不明の外部操作を照合し、履歴を失わず復旧できる。
- crate依存方向が自動テストされ、Domainへframework依存が侵入しない。
- 外部SDK、wire DTO、runtime handle、DB rowがDomain、Application Port、View DTOへ露出しない。
- 外部toolのversion更新を、業務能力が変わらない限りprotocol/Adapter内の変更だけで吸収できる。
- すべての振る舞い変更にRed、Green、最終suiteの`TddCycleEvidence`があり、production codeより先にtestが追加されたことを追跡できる。
- 不具合修正、自動修正、手動調整、依存version更新でもTDD品質ゲートを迂回できない。

この構造により、DDDは「何を管理するか」、関数型設計は「どう安全に判定するか」、Ports and Adaptersは「外界とどう接続するか」、データ指向は「大量の観測データをどう解析・表示するか」をそれぞれ担当する。
