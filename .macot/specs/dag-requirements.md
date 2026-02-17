# DAG実装 要件定義書

## 1. 目的
`macot` の feature execution を「単純な未完了タスク先頭取得」から「依存関係を考慮した実行制御（DAG）」へ拡張し、
以下を実現する。

- 誤った実行順による手戻りの削減
- ブロッカーの可視化
- 大規模タスクでの再現性向上

## 2. スコープ
### 2.1 対象
- `.macot/specs/<feature>-tasks.md` のタスク解析
- 実行対象バッチ選定ロジック
- ブロック状態の判定と表示
- 設定ファイルによる有効/無効切替

### 2.2 非対象
- 外部ワークフローエンジン導入
- 分散実行基盤の追加
- 既存CLIコマンド体系の全面変更

## 3. 用語定義
- **Task**: `- [ ] 1. ...` 形式の1タスク。
- **Dependency**: あるTaskが実行前に完了している必要があるTask番号。
- **Runnable**: 未完了かつ依存先がすべて完了済みのTask。
- **Blocked**: 未完了だが依存未解決のTask。
- **DAGモード**: 依存関係を基にRunnableを選定する実行モード。

## 4. 前提と制約
- タスク番号は既存仕様に合わせ `1`, `2.1`, `3.2.4` のドット記法を許可する。
- タスクファイルは人手編集される前提のため、厳密JSON/YAMLではなくMarkdown解析を継続する。
- 実行基盤は現行の tmux + Claude CLI を維持する。

## 5. 機能要件
### FR-1: 依存関係の記述形式
タスク行末に依存関係メタデータを記述できること。

- 推奨形式: `[deps: <task_no>, <task_no>, ...]`
- 例:
  - `- [ ] 3. API統合 [deps: 1, 2.1]`
  - `- [ ] 4. E2Eテスト [deps: 3]`

### FR-2: タスク解析
パーサはTaskごとに以下を抽出すること。

- task number
- title
- completion state
- dependencies（0件可）

### FR-3: Runnable選定
バッチ選定時、以下条件を満たすTaskのみ選ぶこと。

- 未完了
- dependencies の全task numberが完了済み

### FR-4: バッチサイズ制御
Runnableの先頭から `batch_size` 件を返すこと。

- Runnableが `batch_size` 未満の場合は全件返却
- Runnableが0件かつ未完了Taskが存在する場合は「Blocked状態」と判定

### FR-5: Blocked状態の扱い
Blocked状態を検知した場合、実行ループは完了扱いにしないこと。

- 失敗扱い（Execution Failed）に遷移する
- 失敗メッセージに blocked task番号と不足依存の要約を含める

### FR-6: 互換性
依存メタデータが未記載の既存タスクファイルは従来どおり動作すること。

- dependency未指定Taskは依存なしとしてRunnable判定する

### FR-7: 実行モード設定
設定でDAGモードの有効/無効を切り替え可能にすること。

- `feature_execution.scheduler_mode: dag | sequential`
- 既定値は `dag`

## 6. 非機能要件
### NFR-1: 後方互換
既存設定ファイル（`scheduler_mode` 未指定）で起動エラーにならないこと。

### NFR-2: 可観測性
Tower上の進捗メッセージで、以下が判別できること。

- 実行中バッチ番号
- Blockedによる停止

### NFR-3: 保守性
既存 `feature/task_parser.rs` と `feature/executor.rs` の責務分離を維持すること。

## 7. エラーハンドリング
- `deps` に存在しないtask番号が指定された場合:
  - そのTaskはBlocked扱い
  - エラー詳細に「missing dependency」として表示
- 循環依存がある場合:
  - Runnable 0件 + 未完了ありのためBlocked扱い
  - 明示メッセージで循環依存の可能性を案内

## 8. 受け入れ基準
### AC-1 基本実行順
`2` が `1` 依存の場合、`1` 完了前に `2` はバッチ選定されない。

### AC-2 並列可能タスク
`3` と `4` がともに `2` 依存の場合、`2` 完了後に同一バッチで同時選定される（`batch_size` 範囲内）。

### AC-3 Blocked検知
未完了TaskがあるがRunnableが0件のとき、CompletedではなくFailedになる。

### AC-4 互換動作
`[deps: ...]` を含まないタスクファイルで従来同等の順次処理が継続する。

### AC-5 設定互換
旧設定ファイルをそのまま読み込んでも動作し、既定値でDAGモードになる。

## 9. テスト要件
- Unit
  - `deps` 文字列パース
  - Runnable判定
  - Blocked検知
  - 依存未記載時の互換挙動
- Integration
  - feature execution のフェーズ遷移（SendingBatch -> Polling -> Failed/Completed）

## 10. リスクと対策
- リスク: タスク記法の揺れで依存抽出漏れ
  - 対策: 許容フォーマットを最小に固定し、READMEに明記
- リスク: Blocked理由が不明瞭で運用停止
  - 対策: failed message に task番号と不足依存を必ず含める

## 11. 参考設定例
```yaml
feature_execution:
  batch_size: 4
  poll_delay_secs: 30
  scheduler_mode: dag
```

## 12. 実装完了の定義（DoD）
- 受け入れ基準 AC-1〜AC-5 を満たす
- 新規テストが追加され、関連テストがすべて成功する
- `README.md` または `doc/configuration.md` に依存記法と設定項目が追記される
