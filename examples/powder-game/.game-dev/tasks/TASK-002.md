---
schema_version: 1
id: TASK-002
title: モックTaskの実行フローを確認する
status: ready
contract_revision: 1
acceptance_criteria:
  - AC-002-01
dependencies: []
allowed_paths:
  - .game-dev/mock-output/**
test_paths: []
forbidden_paths:
  - crates/game_runtime/**
risk: low
---

# 目的

Task指示を追加してから、Queue、Agent実行、Local Check、完了確認までの一連の操作を検証する。

# 実装指示

モック成果物として `.game-dev/mock-output/TASK-002.txt` を作成し、次の文字列を1行で記録する。

```text
TASK-002 completed
```

# 完了条件

- `.game-dev/mock-output/TASK-002.txt` が存在する。
- ファイル内容が `TASK-002 completed` と一致する。
- 変更範囲が `allowed_paths` 内に収まっている。
