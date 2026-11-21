# Mitaka Plus RS

macOS、Windows、Linux 向けの非公式天文シミュレーター。現行 macOS 版のコミット [f9fe624](https://github.com/mocho271828/Mitaka_plus/tree/f9fe62485ad45a3b5651e050a49871a71b6f351d) を比較基準に、自前の実装を Rust へ移植しています。

天文データは利用者が用意します。既存の `resources/data` には `hip.dat` と `cf96.dat` が含まれます。画像や探査機のデータは選択したディレクトリの隣から読み取ります。初版の検証対象は macOS 13 以降、Windows 11 x64、Ubuntu 24.04 x64 です。

```sh
cargo build --workspace --locked
cargo test --workspace --locked
```

[レビュー順序](docs/STACK.md)・[実装方針](IMPLEMENTATION_POLICY.md)・[第三者の許諾表示](licenses/Mitaka.txt)

## 星図を起動する

```sh
cargo run --locked -p mitaka-desktop -- --data-root "/absolute/path/to/resources/data"
```

引数を省略した場合は「ファイル」メニューでデータを選びます。「表示」で全天図・地平図を切り替え、ドラッグで移動、スクロールで拡大、クリックで恒星を選択できます。「観測・表示設定」で日時と観測地点を変更します。読み込み結果だけを確かめるには `--inspect --data-root DIRECTORY` を使います。
