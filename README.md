# Mitaka Plus RS

恒星と惑星の位置を指定日時で計算し、地球の空から太陽系・銀河まで探索できる非公式デスクトップアプリです。天文計算、画面、入力処理、Luaホストとメディア操作をRustで実装しています。対象はmacOS 13以降（Apple Silicon・Intel）、Windows 11 x64、Ubuntu 24.04 x64（Wayland・X11）です。

比較基準は既存macOS版のコミット [f9fe624](https://github.com/mocho271828/Mitaka_plus/tree/f9fe62485ad45a3b5651e050a49871a71b6f351d) です。Rust版の取得先は [main](https://github.com/mocho271828/mitaka-plus-rs/tree/main) です。[移植範囲と検証](docs/COMPATIBILITY.md)・[初回移植の統合履歴](docs/STACK.md)・[旧macOS版との差分解消計画とPR](docs/MACOS_PARITY_PLAN.md)

## 配布物から起動する

[Rustワークフロー](https://github.com/mocho271828/mitaka-plus-rs/actions/workflows/ci.yml) で配布物を生成した実行を開き、お使いのOSのArtifactを展開してください。配布の検査結果は各OSの `packages` ジョブで確認できます。利用者が用意したMitakaデータの `resources/data` を「ファイル」→「データを開く…」で選びます。最低限 `hip.dat` と `cf96.dat` が必要です。テクスチャや探査機を使う場合は元の `resources` 以下の配置を保ってください。

| 環境 | 起動方法 |
| --- | --- |
| macOS | Universal ZIPを展開し、`Mitaka Plus.app` を開く。Appleへの公証は行っておらず、配布用アプリはアドホック署名。 |
| Windows | x64 ZIPを展開し、`mitaka-plus` フォルダ内の `mitaka-plus.exe` を開く。Luaとメディアの実行ファイルは同じフォルダに置く。日本語表示には游ゴシックまたはメイリオを使用。 |
| Ubuntu | `sudo apt install ./mitaka-plus_0.1.0_amd64.deb` で導入し、アプリ一覧または `mitaka-plus` から起動する。 |

## 操作

- 「表示」から全天図・地平図・太陽系3Dを切り替えます。日時設定ではUTCやユリウス日を直接入力できます。観測地設定で地点を、表示設定で等級や星座を変更できます。HIP番号を指定すると目的の恒星へ移動します。
- 太陽系3Dはドラッグで回転、Shift＋ドラッグで平行移動、スクロールで拡縮します。「表示」→「銀河カタログ…」で近傍銀河・球状星団・SDSSを選びます。天体ブラウザでは恒星の固有名・HIP番号や太陽・月・惑星の名前を検索できます。小天体、衛星、探査機は日時を変えて位置を追従できます。
- 「操作」→「天体間飛行…」で到着天体と緯度・経度を指定します。目的地を選択したまま中断すると、現在位置から再出発できます。Sキーで地球の地平図から離陸し、太陽系3Dでは選択した惑星・月に離着陸できます。
- 「ファイル」から表示状態（JSON v9）を保存・復元し、画像をPNG・JPEG・BMPで書き出せます。「表示」→「投影ウィンドウ…」で主画面と同じ描画を別ウィンドウに映せます。
- 「ファイル」から開いた画像は独立したプレビューウィンドウで移動・変形できます。
- 「操作」からLuaファイルやコンソールを実行します。Luaは利用者の権限でローカルI/OとOSコマンドを使用できます。ローカルHTMLからの実行許可は開いた文書ごとに管理します。[表示とLua](docs/DISPLAY.md)・[画像・音声・動画](docs/MEDIA.md)

## ソースから起動する

```sh
git clone --branch main https://github.com/mocho271828/mitaka-plus-rs.git
cd mitaka-plus-rs
cargo build --workspace --locked
cargo run --locked -p mitaka-desktop -- --data-root "/path/to/resources/data"
```

Rustの版は `rust-toolchain.toml` に固定しています。LinuxのビルドにはCコンパイラ、pkg-config、WebKitGTK 4.1、D-Bus、X11、Waylandの開発パッケージが必要です。CIと同じパッケージ名は [ワークフロー](.github/workflows/ci.yml) にあります。`--inspect --data-root DIRECTORY` は画面を開かずにデータを検査します。

通常のテストは `cargo test --workspace --locked` で実行できます。画面のある環境では `cargo run --locked -p mitaka-desktop -- --smoke-test --data-root DIRECTORY` が主画面と投影ウィンドウの描画結果を検査します。オフライン用の `*-source.tar.gz` には本体と固定依存のソースが入っています。SDK、Cコンパイラ、指定版Rustを用意して展開し、`cargo build --workspace --frozen` でビルドします。

[実装方針](IMPLEMENTATION_POLICY.md)・[著作権表示](licenses/Mitaka.txt)
