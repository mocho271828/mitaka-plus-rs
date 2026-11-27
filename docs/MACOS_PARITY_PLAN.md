# 旧macOS版との差分解消計画

最優先は、月末の小数秒を含む日時でLuaの月送りが2日ずれる不具合の修正である。これを単独で検証できる最初のPRとし、データ切替、天体ブラウザ、表示設定の復元を続ける。計画に基づいて提出した20件のPRと検証結果を末尾に記録した。

## 比較基準と引き継ぎ

- 対象は `mitaka-plus-rs` の `main@c9091ed` と、移植元 `Mitaka_plus@f9fe624` のmacOS版で利用できた機能。旧Windows版固有の20項目とPro専用の3項目は既存の移植範囲に従って除外する。[移植範囲](COMPATIBILITY.md)・[移植元の基準コミット](https://github.com/mocho271828/Mitaka_plus/tree/f9fe62485ad45a3b5651e050a49871a71b6f351d)
- 照合時に月送りは両版の実関数で再現し、恒星件数は同じ実データで集計した。ほかの差分の根拠はソースとCI定義の照合である。下表の合格条件を、実装時に新たに検証する。
- バックエンドは `vanilla`。1番の親は `main`、2〜20番の親は直前の行のブランチ。各ブランチ名は `codex/parity-` と表の「ブランチ名の末尾」を連結したもの。たとえば2番のPRは `codex/parity-calendar-step` をベースにする。
- 計画保存時点は全20件が `not-started`。実装時の提出範囲は**全20件**（`submitted-now`）。各PRのタイトルに `(Stacked PR)` を付け、親・PRのURL・実測差分・検証結果を記録する。行数はテストを含むレビュー規模の目安とする。

## 1番の回帰条件

入力は **2024-01-31 23:59:59.75 UTC** とする。比較時点のRust版（`c9091ed`）は秒単位に丸めた日時から暦月を判定するため、Luaの月送りが2月29日になる。既存の1月31日正午のテストは両版の規則を区別できるが、丸めで日付をまたぐ条件を含まない。暦日の判定には `mitaka-core` の小数秒を扱える暦変換を使う。[日時変換](../crates/mitaka-desktop/src/main.rs)・[暦ステップと既存テスト](../crates/mitaka-desktop/src/playback.rs)・[コアの暦変換](../crates/mitaka-core/src/astronomy.rs)

| 操作 | 旧版との一致条件 |
|---|---|
| Luaで `Time.step = 7`、続けて `Time:forward()` | **2024-03-02 23:59:59.75 UTC**。移動先の2月1日から30日を加算する。 |
| GUIでUTCオフセットを0にして「1か月」「次へ」 | **2024-02-29 23:59:59.75 UTC**。移動先の月末に日を収め、小数秒を保持する。 |
| 月戻し、うるう日の年送り、固定UTC時差で現地日付が変わる入力 | 移植元の対応する操作とJDが一致する。暦表の範囲を外れる操作では直前のJDを保持する。 |

期待JDは期待日時から独立に生成して実測JDと比較する。計算関数のテストに加え、実際のLuaでも次の操作を通す。小数秒を含む開始時刻は `Time.JD` で与える。

```lua
local quarter_second = 0.25 / 86400
Time.JD = 2460341.5 - quarter_second  -- 2024-02-01 00:00:00 UTC の0.25秒前
Time.step = STEP_MONTH                -- 7
Time:forward()
local expected = 2460372.5 - quarter_second  -- 2024-03-03 00:00:00 UTC の0.25秒前
assert(math.abs(Time.JD - expected) < 1e-9)
```

## 実装順と独立レビューの条件

各行が1つのレビュー単位で、検証はその行のPRに含める。

### 月送り・データ・設定

| 順 | ブランチ名の末尾 | 境界と変更 | このPRの合格条件 | 差分目安 |
|---:|---|---|---|---:|
| 1 | `calendar-step` | 暦計算とLua・GUIの時刻ステップ。丸める前の日時で移動日数を決める。 | 上記の月末・逆方向・年送り・時差・暦表境界の試験が通る。 | 80–150行 |
| 2 | `data-switch` | データ読込。フォルダ確定時にLua、時刻再生・補間、カメラ移動など旧データで始まった操作を止め、バックグラウンドで読み込む。 | 同じJDが新旧双方で有効でも再生は停止し、旧Luaの遅延操作が新データに届かない。連続読込では最新の要求だけを反映し、失敗時は直前のデータを停止状態で表示する。 | 250–450行 |
| 3 | `catalog-search` | 天体ブラウザ。全HIPに対応する固有名があれば結合し、太陽系11天体・恒星・星座・NGC/IC・SDSS銀河・クエーサーを横断検索する。 | 実データでHIP 117,956件を対象にし、固有名表174件に載らない恒星にも移動できる。太陽・月・惑星の日英名、結果からの中央表示、ページ切替、古い検索結果の破棄を確認する。 | 350–600行 |
| 4 | `display-preferences` | 終了・起動時の表示設定。旧版の保存項目を自動保存し、次回のデータ読込成功後に一括復元する。明示操作の `remembered-v9.json` は別に扱う。 | 同じデータで再起動すると星座・座標線・時刻表示が戻る。保存値の破損やデータ不一致でも部分適用や上書きが起きず、元の設定を保持する。 | 200–400行 |
| 5 | `settings-windows` | 日時・観測地の設定画面。GUIとLuaの開閉状態を各画面に結び付ける。 | 両方を開き、それぞれ片方だけを閉じる。`GUI.timeSetting`・`GUI.placeSetting` の取得値が画面の状態と一致する。 | 100–220行 |
| 6 | `julian-input` | 日時設定のJD入力。Enterまたは適用で既存の日時検証を通す。 | 正常値でUTC表示も更新する。不正値・非有限値・暦表範囲外では直前のJDを保持し、入力欄も元に戻る。 | 60–150行 |
| 7 | `surface-zoom` | Luaのカメラ距離と補間。停止中の3D地表で距離ズームを拒否する。 | 月・火星で距離操作が拒否され、操作前のカメラ位置・姿勢を保つ。宇宙視点・飛行中の距離操作と画角操作を旧版の条件と照合する。 | 60–140行 |

### 星図・太陽系・ローカル媒体

| 順 | ブランチ名の末尾 | 境界と変更 | このPRの合格条件 | 差分目安 |
|---:|---|---|---|---:|
| 8 | `sky-input` | 全天図・地平図の入力。クリック選択中のダブルクリックで天体を中央に表示し、2本指スクロールで縦横にパンする。 | 両図で中央表示、両軸パン、ホイールとピンチのズームを検証する。地平図では地形による遮蔽を反映する。 | 140–280行 |
| 9 | `earth-moon-view` | 太陽系パネルの地球・月接近表示。初期距離は両天体の間隔の2.6倍、下限は0.001 AUとする。 | 日時変更後も両天体の中点を追従し、複数日時で両方が画面内に収まる。 | 180–350行 |
| 10 | `minor-body-orbits` | 小惑星・太陽系外縁天体の軌道線。拡大時に現在位置付近を投影誤差に応じて再標本化する。 | 旧版の比較ケースで本体と線のずれが0.6画素未満になる。データ欠損区間には線を引かない。 | 160–320行 |
| 11 | `lunar-orbit` | 月軌道の標本化。27.321661日・両端込み181点に揃える。 | 暦表中央と両端で、標本数・先頭位置・末尾位置を旧版と比較する。 | 50–120行 |
| 12 | `moon-mercury-terrain` | 地形読込と描画。選択したデータ直下の `moon_topo.dat` と `mercury_topo.dat` を月・水星の地形に接続する。 | 合成地形で両天体の標高と描画を確認する。任意ファイルがない場合も通常の天体表示を継続する。 | 120–260行 |
| 13 | `surface-sdss` | 3D地表でのSDSS描画条件。銀河・クエーサーの描画を地表滞在中だけ抑止する。 | 両方の設定を有効にしたまま着陸・離陸すると、設定値が保持され、離陸後に表示が戻る。 | 50–120行 |
| 14 | `console-input` | Luaコンソールの入力制限。65,536文字の打ち切りを解消し、実行時にUTF-8のバイト数を判定する。 | 日本語を含む1 MiBちょうどの入力を実行できる。1バイト超過では実行を拒否し、入力全文が残る。 | 60–140行 |
| 15 | `image-window` | 画像プレビューの独立ウィンドウ。別モニターに移動できるようにする。 | 画像の変形・不透明度、ウィンドウの開閉とLuaの `visible` の同期、データ切替時の終了を確認する。 | 140–280行 |
| 16 | `html-layout` | ローカルHTMLのレイアウト。既存の描画に本文・セルの文字揃えと枠線を反映する。 | 合成文書で配置・描画とリンクのクリック領域を検証し、文書ごとのLua実行許可も確認する。 | 150–300行 |

### 配布・検証・案内

| 順 | ブランチ名の末尾 | 境界と変更 | このPRの合格条件 | 差分目安 |
|---:|---|---|---|---:|
| 17 | `packaged-gui` | 配布CI。3 OSの配布ZIP・debから取り出した本体に対して `--smoke-test` を実行する。 | 展開済み実行ファイルから主画面と投影画面を描画できる。UbuntuはWaylandとX11の両方を通す。 | 50–120行 |
| 18 | `offline-build` | オフライン用ソース配布。生成したアーカイブを別ディレクトリに展開してビルドする。 | 固定ツールチェーン、空の `CARGO_HOME`、新規ビルド先で `cargo build --workspace --frozen` が完走する。 | 40–100行 |
| 19 | `video-seek` | Windows・Ubuntuの実プレーヤー試験。6秒・90フレームの固定フレームレート動画を使う。 | 再生・一時停止後にフレーム番号45へ移動し、プレーヤーが報告するフレーム番号と時刻3秒が一致する。 | 60–160行 |
| 20 | `main-docs` | 配布・操作案内。READMEの取得先を `main` に直し、旧スタックの#7〜#58が `c9091ed` で一括統合された履歴を記す。 | クローン手順とスタック表が `main` に対応し、操作・媒体・互換性の案内から検証済みの環境と実機の未確認項目を判別できる。 | 70–160行 |

## 共通の検証と提出記録

- 各PRの自動試験は合成データで実行する。利用者の天文データは実機での照合に読み取り専用で使い、原本・出典・許諾表示を保全する。[実装方針](../IMPLEMENTATION_POLICY.md)・[著作権表示](../licenses/Mitaka.txt)
- 各単位で該当するCargoのビルド・テスト・フォーマット・Clippyを確認する。先端では `cargo build --workspace --locked`、`cargo test --workspace --locked`、`cargo fmt --all -- --check`、`cargo clippy --workspace --all-targets --locked -- -D warnings` と3 OSの配布試験を通す。
- 実機ではWindows 11、Intel Mac、複数モニター間での画像移動、実スピーカー出力と再生中の装置切断を検証する。OS・機器・操作・観測結果を記録し、機材を確保できない項目には「未検証」と明記する。[検証環境](COMPATIBILITY.md)・[媒体の検証範囲](MEDIA.md)
- 作業単位ごとにGit状態・変更ファイル・差分量を確認し、3つのpolicy gateの作業コピーを日時別に判定する。配布GUI・オフラインビルドの合格条件は17・18番の変更と一緒に検証方針へ反映する。[開発時の規則](../AGENTS.md)・[ビルドと検証のgate](../policy_gates/testing.md)
- レビューで見つけた修正は、その責務を持つ行に戻す。1つの行に別々のレビュー理由が生じたら分割し、順序や親が変わる場合は先にこの表を更新する。旧スタックの履歴は別文書で管理する。[旧スタック表](STACK.md)

## 提出したPR

下表の親・差分・個別の検証は、20件のドラフトPRを初めて提出した時点の記録。差分はテキスト行数で、バイナリは集計に含めない。レビューで追加した修正・回帰テストと最終的なマージ先は、各PRのリンクから確認できる。

| 順 | PR | 親 | 差分 | 個別の検証 |
|---:|---|---|---:|---|
| 1 | [#59](https://github.com/mocho271828/mitaka-plus-rs/pull/59) | `main` | +54 / −8 | 修正前の入力で期待値との2日差を再現。修正後に月・年・逆送り・時差を含むplaybackテスト5件、workspace build・Clippy・fmt、合成暦表を使ったmacOS GUI・実Luaスモークを通過。 |
| 2 | [#60](https://github.com/mocho271828/mitaka-plus-rs/pull/60) | `codex/parity-calendar-step` | +237 / −46 | macOSでdesktop単体78件、fmt・build・Clippyが成功。実Luaと同じJDを保つデータ切替、失敗時の旧データ保持、連続要求の最新優先を合成GUIスモークで検証。 |
| 3 | [#61](https://github.com/mocho271828/mitaka-plus-rs/pull/61) | `codex/parity-data-switch` | +633 / −114 | 実HIP 117,956件と固有名174件、無名HIPの検索と中央座標を検査。合成カタログで6種横断、全角・半角・英日名、500件ページと古い検索の破棄を検証した。 |
| 4 | [#62](https://github.com/mocho271828/mitaka-plus-rs/pull/62) | `codex/parity-catalog-search` | +262 / −35 | 一時設定ディレクトリで終了時の保存とデータ読込後の復元、データ待ち・不正JSON・カタログ不一致時の非上書き、独立した手動記憶と観測地保持を合成GUIで確認した。別プロセスでアプリの終了処理を通し、次回起動時の復元も確認した。 |
| 5 | [#63](https://github.com/mocho271828/mitaka-plus-rs/pull/63) | `codex/parity-display-preferences` | +126 / −82 | macOSの合成GUIで実際のLuaから両設定を開き、相互に片方ずつ閉じて取得値を照合した。 |
| 6 | [#64](https://github.com/mocho271828/mitaka-plus-rs/pull/64) | `codex/parity-settings-windows` | +57 / −2 | macOS GUIスモークで暦表初日の入力、不正文字列・NaN・無限値・上限外の入力、入力欄とUTC・JDの復元を確認した。 |
| 7 | [#65](https://github.com/mocho271828/mitaka-plus-rs/pull/65) | `codex/parity-julian-input` | +33 / −1 | 合成GUIで火星地表の拒否・離陸中の許可を確認。実Luaで月面の直接・線形・対数距離指定が拒否され、距離・姿勢を維持して画角を変更できた。 |
| 8 | [#66](https://github.com/mocho271828/mitaka-plus-rs/pull/66) | `codex/parity-surface-zoom` | +161 / −26 | 合成スクロール・ダブルクリック入力と、実データの地平図の遮蔽・探査機選択を検証した。 |
| 9 | [#67](https://github.com/mocho271828/mitaka-plus-rs/pull/67) | `codex/parity-sky-input` | +165 / −44 | 合成座標で初期構図、実CF96で1日・7日・365日後の中点追従と両天体の画面内への投影を検査した。 |
| 10 | [#68](https://github.com/mocho271828/mitaka-plus-rs/pull/68) | `codex/parity-earth-moon-view` | +249 / −7 | 合成楕円と実データのCeres・Sednaの3日時で投影誤差0.6画素未満、欠落区間・標本数を確認した。 |
| 11 | [#69](https://github.com/mocho271828/mitaka-plus-rs/pull/69) | `codex/parity-minor-body-orbits` | +75 / −22 | 旧macOS版のtest_solar_modelとRust版に実データを入力し、暦表の開始・中央・終端で181標本と両端位置を確認した。 |
| 12 | [#70](https://github.com/mocho271828/mitaka-plus-rs/pull/70) | `codex/parity-lunar-orbit` | +182 / −57 | 月・水星の合成400 m地形とファイルなしの両条件で、3D描画深度と標高を実測した。既存の実地球・火星地形の描画検査も成功。 |
| 13 | [#71](https://github.com/mocho271828/mitaka-plus-rs/pull/71) | `codex/parity-moon-mercury-terrain` | +70 / −4 | 銀河・クエーサー各設定、地表滞在後の表示復帰、近傍銀河の継続表示を検証した。実SDSS描画選択試験にも成功。 |
| 14 | [#72](https://github.com/mocho271828/mitaka-plus-rs/pull/72) | `codex/parity-surface-sdss` | +51 / −16 | 日本語を含む1 MiBを全文保持して準備でき、1バイト超過を履歴登録前に拒否することを単体試験で確認。1 MiBの実Luaプログラムも実行できた。 |
| 15 | [#73](https://github.com/mocho271828/mitaka-plus-rs/pull/73) | `codex/parity-console-input` | +173 / −27 | 独立した子viewportの描画メッシュ・画像変形・開閉・Lua状態とデータリセットを検査した。物理的な複数画面での移動は未検証。 |
| 16 | [#74](https://github.com/mocho271828/mitaka-plus-rs/pull/74) | `codex/parity-image-window` | +560 / −143 | 合成HTMLと元のローカルパネルを読み、文字揃えと罫線の描画・折返し後のクリック領域・CSS変更時の実行許可の取消を検査した。 |
| 17 | [#75](https://github.com/mocho271828/mitaka-plus-rs/pull/75) | `codex/parity-html-layout` | +8 / −3 | macOSでUniversal ZIPを作成・展開し、合成HIP 4,096件の読込、主画面と投影のGPU読み戻しを確認した。ワークフローとシェル構文も検査した。 |
| 18 | [#76](https://github.com/mocho271828/mitaka-plus-rs/pull/76) | `codex/parity-packaged-gui` | +19 / −4 | macOS arm64で生成したtar.gzを再展開し、新規CARGO_HOME・CARGO_TARGET_DIRでworkspaceのfrozenビルドが完走した。 |
| 19 | [#77](https://github.com/mocho271828/mitaka-plus-rs/pull/77) | `codex/parity-offline-build` | +30 / −11 | macOS arm64の実プレーヤーで合成H.264のフレーム番号45/90、PCM WAVの48,000/96,000がいずれも3秒に一致した。Windows・Ubuntuの検査を通常CIと配布CIに接続した。 |
| 20 | [#78](https://github.com/mocho271828/mitaka-plus-rs/pull/78) | `codex/parity-video-seek` | +178 / −68 | READMEの取得手順、旧52件のPRリンクと一括統合履歴、実機未検証の範囲を確認した。 |
