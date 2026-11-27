# mitaka-script

ユーザーが選んだローカルファイルを、独立した Lua 5.4.9 プロセスで実行します。

```sh
cargo build -p mitaka-script --release
cargo run -p mitaka-script -- path/to/program.lua
cargo test -p mitaka-script
```

単独起動時の相対パスはスクリプトのディレクトリが基準です。`--working-dir` で変更できます。標準入出力を使用でき、Ctrl+C で停止します。成功時の終了コードは 0、読込・評価エラーは 1、引数エラーは 2 です。GUI のコンソールは `--console` で式と文を評価し、式の結果と `print` の出力を表示します。実行中のスクリプトはコンソールから停止できます。

Lua 標準ライブラリの io・os・package・debug を含み、実行中のスクリプトはユーザー権限でファイルや外部プログラムを操作できます。スクリプト上限は 1 MiB、Lua メモリは 64 MiB、命令フックは約 1,000 万命令、待機を含む通常予算は 110 秒です。ヘルパーに 120 秒の終了期限があります。既定起動時にスクリプトを自動実行する処理はありません。

## ローカルホストとの通信

ホストがローカルの接続先を用意してから `mitaka-script --host ENDPOINT FILE.lua` を起動します。Unix ではファイル型 Unix socket のパス、Windows ではローカル named pipe の名前を渡します。GUI から起動した子プロセスの標準入出力は無効にし、ログはホスト接続で受け取ります。プロトコルは Mitaka_plus コミット `f9fe62485ad45a3b5651e050a49871a71b6f351d` の script_transport.cpp、ScriptWorker.mm、ScriptController.h/.mm に対応した、4 バイト big-endian 長＋UTF-8 JSON（最大 65,536 バイト）です。接続方法はこの Rust 版のものです。

- snapshot は GUI の現在値を読む要求です。応答の ok と要求キーの型を検証します。
- apply は元の整数 kind、number、enabled、text、duration、hips、radiusScale、orientation を渡します。ホストからの拒否は Lua エラーとして返ります。
- print はホスト接続時に log として転送します。最後の done は応答を待ちません。
- 単独実行では距離・時間ステップ定数と `waitTime` を利用できます。GUI 接続時は `Time` で UTC の年月日時分秒を操作でき、「Lua の地方時差を指定」を有効にすると地方時も使えます。`Time:setJDTo(2451545, 2)` は2秒で指定のユリウス日まで進めます。`Camera` は方位・視野・注視対象の変更や、`Camera:setEulerTo(10, 20, 30, 2)` のような姿勢の補間に対応します。
- 星名・星座の選択、惑星の表示倍率、登録メディアの表示や再生は GUI と状態を共有します。対応する旧 API と利用条件は各ガイドを参照してください。[星図の表示と観測地点](../../docs/DISPLAY.md)・[ローカル媒体](../../docs/MEDIA.md)・[移植範囲と検証](../../docs/COMPATIBILITY.md)

## 依存と出典

同梱アーカイブの SHA-256 は `2335b6c582a52654f94612bf10d2f4672805d05329aa6568b1d8cd9e5c6fb8e6` です。build.rs は展開前に検証し、Lua の C ソースを cc で対象 OS 向けに静的リンクします。ビルドには Rust と対象 OS の C ツールチェーンが必要です。[Lua の許諾表示](vendor/LUA_LICENSE.txt)を同梱してください。mlua で VM を操作し、interprocess でローカル IPC、command-group で停止時のプロセスグループを扱います。
