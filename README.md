# kotomemo-native

このアプリは、kotomemoのネイティブ動作を試すRust版をつくってみたものです。

**kotomemo-native** は、Rust（`eframe` / `egui`）で実装された軽量・高速なタブ型テキスト／メモエディタです。JVM ランタイム不要の単一ネイティブバイナリとして動作し、日常のメモ取りからコード断片の編集、HTTP API（LLM・Webhook・各種バックエンド）との連携までシンプルに行えます。

---

## 主な機能

### エディタ基本機能
- **マルチタブ＆マルチウィンドウ編集**
  - 未保存マーカー（`*`）付きのタブ管理
  - タブのドラッグ＆ドロップによる並び替え、ウィンドウ外へのドラッグによる**別ウィンドウへの分離**、別ウィンドウへのドロップによる**タブの統合**
  - **タブの並列表示（Tile Tabs）**: `Ctrl+クリック` / `Shift+クリック` で複数タブを選択し、横に並べて比較・同時編集が可能
- **シングルインスタンス＆エクスプローラ連携**
  - Windows エクスプローラの右クリックメニュー（`Open with kotomemo`）やコマンドラインから複数ファイルを同時に開いた場合、起動中の最新ウィンドウに別タブとして展開
- **テキスト編集・表示**
  - 行番号表示トグル（`View` -> `Show Line Numbers`）
  - 右端での折り返しトグル（`View` -> `Word Wrap` / `Alt+Z`）
  - ファイル末尾が画面中央付近までスクロール可能な余白（オーバースクロール）
  - 複数行選択時のブロックインデント／アウトデント（`Tab` / `Shift+Tab`）
  - 正規表現（Regex）・大文字小文字区別・選択範囲スコープに対応した検索・置換（`Ctrl+F` / `Ctrl+H`）
  - 1行目のテキストからファイル名を自動提案する保存ダイアログ
  - シンタックスハイライト（C系言語、スクリプト/`#`コメント系、XML/HTML、SQL のコメント・文字列強調）
- **文字コード・改行コード**
  - 文字コード自動判定・変換（`UTF-8` / `UTF-16LE` / `UTF-16BE`）
  - BOM（Byte Order Mark）の付与・解除トグル
  - 改行コード（`LF` / `CRLF`）の自動判定・切り替え
- **フォントカスタマイズ**
  - 日本語と ASCII 文字のウェイト・字形を統一（Noto Sans JP を標準内蔵）
  - システムフォント（BIZ UDGothic / BIZ UDPGothic / メイリオ / 游ゴシック / MS ゴシック等）や任意の `.ttf` / `.otf` / `.ttc` ファイルの選択、ズーム操作（`Ctrl+=` / `Ctrl+-` / `Ctrl+0`）

### 画像添付（Attachments）
- クリップボードからの画像貼り付け（`Ctrl+V`）および画像ファイルのドラッグ＆ドロップに対応
- 編集中のファイルと同じ階層の `attachments/` フォルダに PNG として自動保存し、カーソル位置に `[img: ...]` リンクを挿入（下部 Images ビューでプレビュー表示可能）

### HTTP API 連携（Send to API）
- 選択範囲（未選択時は文書全体）を任意の HTTP エンドポイント（LLM API、Webhook 等）へ送信（`Ctrl+Enter` / `Ctrl+Shift+P`）
- プリセットごとに URL、HTTP メソッド（`GET` / `POST` / `PUT` / `PATCH` / `DELETE`）、ヘッダ、ボディテンプレートを設定可能
- テンプレート変数: `{{selection}}`, `{{selectionJson}}`, `{{filename}}`, `{{tokens.NAME}}`
- レスポンスの JSON パス抽出（例: `choices.0.message.content`）と出力先選択（新規タブ / 選択範囲の直後に挿入 / ステータス表示のみ）
- API トークンは OS 標準のセキュアストレージ（Windows 資格情報マネージャー / macOS Keychain / Linux Secret Service）に保存（利用不可環境では `~/.kotomemo/config` にフォールバック）

---

## 対応OSと起動コマンド

### 対応OS
- **Windows** のほか、**Linux / macOS でもソースからビルドすればそのまま実行可能**です（マルチプラットフォーム対応）。

### 起動コマンド（`kotomemo`）
> **Note**: リポジトリ名は `kotomemo-native` ですが、生成される実行バイナリ名および起動コマンドは **`kotomemo`**（Windows では `kotomemo.exe`）です。インストーラ導入時や `PATH` を通した環境では `kotomemo` コマンドで起動できます。

```bash
# アプリを単体で起動
kotomemo

# ファイルを指定して起動（存在しないファイル名を指定した場合は新規作成対象として開きます）
kotomemo path/to/note.txt

# 複数ファイルを同時に指定して起動（同一ウィンドウの別タブとして開きます）
# ※すでに kotomemo が起動している場合は、最新のウィンドウに新しいタブとして追加されます
kotomemo file1.txt file2.txt
```

---

## ビルドと開発用コマンド

### 必要環境
- [Rust](https://rustup.rs/)（Edition 2024 対応の最新 stable ツールチェーン）
- Linux（Ubuntu / Debian 系）でビルドする場合のみ、ファイルダイアログ（GTK3）用の開発パッケージが必要です：
  ```bash
  sudo apt-get update && sudo apt-get install -y libgtk-3-dev
  ```

### ビルド手順

```bash
# ユニットテストの実行
cargo test

# リリースビルド（Windows / Linux / macOS 共通）
# 生成先: target/release/kotomemo（Windows は target/release/kotomemo.exe）
cargo build --release

# ビルドしたバイナリを直接実行
./target/release/kotomemo

# Cargo 経由での開発起動（引数を渡す場合は -- の後に指定）
cargo run
cargo run -- path/to/file.txt
```

### インストーラ／パッケージの作成
[`cargo-packager`](https://github.com/crabnebula-dev/cargo-packager) を使用して各 OS 向けのインストーラを生成できます。

```bash
cargo install cargo-packager --locked
cargo packager --release
```

---

## ライセンス

- 本ソフトウェアのソースコードは [MIT License](LICENSE) の下で公開されています。
- 同梱フォント `assets/fonts/NotoSansJP-Regular.otf` は [SIL Open Font License 1.1](assets/fonts/NotoSansJP-LICENSE.txt) の下で提供されています。
