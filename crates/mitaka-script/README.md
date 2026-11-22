# mitaka-script

ユーザーが選んだローカルファイルを、独立した Lua 5.4.9 プロセスで実行します。

```sh
cargo build -p mitaka-script --release
cargo run -p mitaka-script -- path/to/program.lua
cargo test -p mitaka-script
```

相対パスは選択したスクリプトのディレクトリを基準にします。通常の print、io.stdin、io.stdout、io.stderr が使えます。成功時は終了コード 0、読込・評価エラーは 1、引数エラーは 2 です。コマンドラインからは Ctrl+C で停止できます。GUI からは spawn_helper の返すプロセスグループを kill、wait の順に呼んで回収します。

Lua 標準ライブラリの io・os・package・debug を含み、実行中のスクリプトはユーザー権限でファイルや外部プログラムを操作できます。スクリプト上限は 1 MiB、Lua メモリは 64 MiB、命令フックは約 1,000 万命令、待機を含む通常予算は 110 秒です。ヘルパーに 120 秒の終了期限があります。既定起動時にスクリプトを自動実行する処理はありません。

## ローカルホストとの通信

ホストがローカルの接続先を用意してから `mitaka-script --host ENDPOINT FILE.lua` を起動します。Unix ではファイル型 Unix socket のパス、Windows ではローカル named pipe の名前を渡します。標準入出力はスクリプトが使用できます。プロトコルは Mitaka_plus コミット `f9fe62485ad45a3b5651e050a49871a71b6f351d` の script_transport.cpp、ScriptWorker.mm、ScriptController.h/.mm に対応した、4 バイト big-endian 長＋UTF-8 JSON（最大 65,536 バイト）です。接続方法はこの Rust 版のものです。

- snapshot は GUI の現在値を読む要求です。応答の ok と要求キーの型を検証します。
- apply は元の整数 kind、number、enabled、text、duration、hips、radiusScale、orientation を渡します。ホストからの拒否は Lua エラーとして返ります。
- print はホスト接続時に log として転送します。最後の done は応答を待ちません。
- 距離・時間ステップ定数と waitTime は単独でも利用できます。ホスト接続時は Time.JD/step/stepValue/speed、Camera の target/zoom/fov/azimuth/altitude/mode/duration など、59 エントリの表示フラグと toggle、Time の停止・単発移動等を扱えます。全契約は src/bindings.rs に列挙されています。各操作の結果は受け手の GUI が対応する機能に依存します。
- 日付成分の操作、姿勢・時間付きの補間、コンソール式評価、全ての旧 Lua API には未対応です。Lua 5.1 専用機能は補完していません。

## 依存と出典

同梱アーカイブの SHA-256 は `2335b6c582a52654f94612bf10d2f4672805d05329aa6568b1d8cd9e5c6fb8e6` です。build.rs は展開前に検証し、Lua の C ソースを cc で対象 OS 向けに静的リンクします。ビルドには Rust と対象 OS の C ツールチェーンが必要です。[Lua の許諾表示](vendor/LUA_LICENSE.txt)を同梱してください。mlua で VM を操作し、interprocess でローカル IPC、command-group で停止時のプロセスグループを扱います。
