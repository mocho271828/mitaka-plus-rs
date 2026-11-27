# ローカル媒体の登録とプレビュー

「ファイル」→「ローカル媒体の登録…」は、選択した `resources/data` の隣にある `media/images.dat`、`images_list.dat`、`movies.dat`、`audio.dat` を読み込みます。二つの画像登録は同じ行の出現数だけ相殺します。UTF-8 と CP932 を受け付け、MVP の最終行を単独 PC 用の参照先として表示します。未知の種類・曖昧な名前・未解決の参照も一覧に残ります。

一覧の「このファイルを表示」で選択した媒体を開きます。「参照ファイルを指定…」は未解決の項目を実ファイルに関連付けます。「画像・音声・動画を開く…」からもファイルを追加できます。登録を読み込んだだけのファイルを Lua は表示できません。アプリは手動選択した通常ファイルの読み取りハンドルを保管し、それを画像デコーダーまたは子プレーヤーに直接渡します。参照の自動解決は選択したツリー内に限ります。データの切り替え・アプリ終了で選択の権限は破棄します。

JPEG・PNG は別モニターに移動できる独立ウィンドウで表示します。画像の位置・倍率・回転・不透明度を調整できます。64 MiB、約6710万画素まで読み、長辺1024画素に収まる表示用画像を生成します。原本は書き換えません。

動画・音声は同梱の `mitaka-media` で再生します。シークと再生・一時停止はプレビューやプレーヤーで操作できます。表示した種類の Lua オブジェクトも同じ再生状態を読み取ります。

| Lua | 操作 |
| --- | --- |
| `Images`、`Movies`、`Audio` | `name`、種類別の0始まり `index`、`visible`、`toggle()`、`first()`、`prev()`、`next()`、`last()`。重複名は番号で選択。表示は1媒体ずつ。 |
| `Movies`、`Audio` | `playing`、`play()`、`pause()`、`togglePlaying()`。操作は非同期、状態はデコーダーが報告した実測値。 |
| `Movies.frame`、`Audio.frame` と読取専用 `frameLength` | PCM WAV は1チャンネル当たりの標本フレーム。256 MiB 以下の非フラグメント MP4 は表示時刻の等間隔性とデコーダーの継続時間を照合できた単一動画トラックのみ。可変フレームレートなど未確定の形式は秒単位のGUIシークを利用。 |
| `GUI.soundControler` / `GUI:toggleSoundControler()` | 再生中の音声を操作するパネルを表示・非表示。綴りは旧版のまま。 |

現行 macOS 比較版 f9fe624 の MEDIA-01〜03 は手動の動画・音声・画像プレビューと登録一覧に対応します。Rust 版はそこへ旧 Windows 版説明書の Movies／Audio／Images と上記の操作を接続しました。旧版フレーム単位の全形式での一致、標準画面への映像合成、MEDIA-04 の旧3Dチャート、MEDIA-05 の旧タイトル演出、画像変形用の独自 Lua プロパティは実装していません。MEDIA-06 は音声プレビューの標準操作と上記パネルに対応します。画像登録のルビは原文を表示します。

## 再生環境

| OS | API と配布条件 |
| --- | --- |
| macOS 13 以降 | WKWebView と OS のデコーダー。`Contents/MacOS/` の Universal プレーヤーを使用。 |
| Windows 11 x64 | WebView2 Evergreen Runtime と Windows のメディアデコーダー。ZIP に同梱した exe を同じフォルダに置く。Windows N は Media Feature Pack が必要な場合があります。 |
| Ubuntu 24.04 x64 | WebKitGTK 4.1 と GStreamer。ビルドは `libwebkit2gtk-4.1-dev libdbus-1-dev`、deb の実行は GStreamer base/good、コーデックに応じ libav を使用。 |

プレーヤーのループバックサーバーが返すのは、固定の再生ページ、同梱の Rust/Wasm 制御、選択済みファイルの指定範囲だけです。ユーザーの HTML や Lua をページとして読み込む入口はありません。ブラウザー内のコードは Rust で実装し、wasm-bindgen の固定版が標準 JavaScript バインディングを生成します。ページには再生ごとのランダムな URL を使い、別の URL へのナビゲーション・新規ウィンドウを禁止します。親アプリの制御用トークンはページに渡しません。

## ビルドと確認

通常は `cargo build --locked --workspace --bins` でデスクトップとプレーヤーをビルドします。ビルド時は同梱 JavaScript/Wasm の SHA-256 を確認します。生成時は対象Wasmクレートの識別子をロック済みのパッケージ名と版から決めます。これによりLinux x86_64とmacOS arm64で生成バイトを照合できます。固定版 Rust 1.100.0 の `wasm32-unknown-unknown` 標準ライブラリを準備して `bash tools/media-web.sh --check` を実行すると、Rust ソースから両ファイルを再生成してバイト単位で比較します。変更時は `--write` を使います。生成ライブラリ・Rust 側の依存はルートの Cargo.lock とオフライン配布の vendor に含まれます。Rust ツールチェーンと Wasm ターゲットの標準ライブラリは事前に導入してください。

```sh
cargo test --locked --workspace
MITAKA_DATA_ROOT=/path/to/resources/data cargo test --locked -p mitaka-desktop native_registration_baseline -- --ignored --nocapture
MITAKA_TEST_MEDIA_HELPER=/path/to/mitaka-media cargo test --locked -p mitaka-desktop real_player_controls_the_selected_handle -- --ignored
```

最後の試験は画面のある環境で6秒の16 kHz PCM WAVを生成し、同梱の合成H.264 MP4（15 fps、6秒・90フレーム）も開きます。両方とも実プレーヤーで再生・一時停止して3秒に移動し、WAVは48,000フレーム目、MP4は45フレーム目が返ることを検査します。`MITAKA_TEST_MEDIA_VIDEO` を指定すると同条件の別の動画を使用できます。macOS arm64のWKWebViewでこの操作を確認しました。UbuntuのCIはXvfbとPulseAudioの仮想出力を使い、WindowsのCIはWebView2のテストプロセスで仮想音声出力を指定します。実スピーカーと再生中の装置切断は実機での確認項目です。

動画の入力はFFmpegの合成パターンです。リポジトリのルートで次のコマンドを使って同条件の動画を生成できます。

```sh
ffmpeg -f lavfi -i testsrc2=size=64x48:rate=15 -frames:v 90 -an -c:v libx264 \
  -profile:v baseline -pix_fmt yuv420p -crf 30 -threads 1 -movflags +faststart \
  -bsf:v filter_units=remove_types=6 -map_metadata -1 crates/mitaka-desktop/tests/fixtures/seek-15fps.mp4
```
