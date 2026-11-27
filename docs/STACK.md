# Rust移植のレビュー順序

このチャットで確定した「mitaka-plus-rs を新設し、3 OS対応のRust版を実装する」を採用する。移植基準は mocho271828/Mitaka_plus の f9fe62485ad45a3b5651e050a49871a71b6f351d。計画は同一セッションにあり、元ファイルはない。対象、数値・データ・Luaの契約、対象環境、各段階の検証方法を確認した。Gitのバックエンドは vanilla、基点は main。

各ブランチは直前の行を親とする。単位ごとの検証だけで記載した判定ができるため、独立レビュー可とした。先頭6件を submitted-now、それ以降は local-only とする。後続の検証は同じフィーチャーブランチの積み上げで実施する。状態とPRは実績に応じて記録する。

| 順 | ブランチ（codex/ 以下） | レビュー理由・対象 | 検証と独立判定 | 目安 | 状態 |
|---|---|---|---|---|---|
| 1 | foundation | infra・独立ビルド、出典、方針、3 OS CI | 新規クローンのCargoビルドとテストを再現 | 250行 | [submitted #1](https://github.com/mocho271828/mitaka-plus-rs/pull/1) |
| 2 | catalogs | domain・HIPと星座の原行・破損検出 | 合成データ、実hip.datの件数 | 600行 | [submitted #2](https://github.com/mocho271828/mitaka-plus-rs/pull/2) |
| 3 | astronomy | domain・日時、座標、惑星暦と観測 | 固定JDの位置、暦表の端、天頂・地平線の試験 | 1400行 | [submitted #3](https://github.com/mocho271828/mitaka-plus-rs/pull/3) |
| 4 | script-runtime | adapter・固定Luaを実行する別プロセス | 原本のハッシュとローカルスクリプトの実行・失敗 | 650行＋Lua原本 | [submitted #4](https://github.com/mocho271828/mitaka-plus-rs/pull/4) |
| 5 | sky-desktop | ui・共通の星図、観測地点、操作 | 実データ、投影・対象選択、Macでの起動 | 1300行 | [submitted #5](https://github.com/mocho271828/mitaka-plus-rs/pull/5) |
| 6 | distribution | infra・各OS用の配布物 | ZIPとdebの内容、展開後の起動、許諾 | 400行 | [submitted #6](https://github.com/mocho271828/mitaka-plus-rs/pull/6) |
| 7 | lunar | domain・月の地心位置と地月重心の補正 | 原版由来の位置・姿勢とCPU別丸め誤差を検証 | 700行 | implemented locally |
| 8 | terrain | domain・地形のバイナリ索引と標高 | 切断や負標高の合成試験、実データの火星標高 | 500行 | implemented locally |
| 9 | solar-scene | ui・太陽系と地形・視点移動 | 実データの地球・月を含む11天体と火星地形、飛行・画像出力 | 950行 | implemented locally |
| 10 | galaxy-catalogs | domain・球状星団・銀河・恒星名のカタログ | 固定幅/件数・原バイトの合成試験と実データ | 850行 | implemented locally |
| 11 | deep-sky | ui・銀河の探索と深宇宙表示 | 検索・選択と実画素での描画を検証 | 800行 | implemented locally |
| 12 | satellites | domain・衛星データと参照座標 | 衛星カタログの親子関係と向き・軌道を既存値で検証 | 1400行 | implemented locally |
| 13 | session-v9 | contract・セッション読込・保存 | 旧版で生成したv9を往復し範囲外や不完全な復元を拒否 | 800行 | implemented locally |
| 14 | local-scripting | adapter・GUIからのLuaとローカルHTML操作 | 手動操作・停止・状態適用・信頼の解除 | 1000行 | not-started |
| 15 | surface-optics | 天体の自転、食・大気の光学計算 | 原版の数値と光度・遮蔽の境界、実データ | 1800 | not-started |
| 16 | playback | 日時の再生・逆再生とカレンダー | 境界停止、月末の扱いとLua経由の時刻設定 | 450 | not-started |
| 17 | planet-surfaces | 惑星表面のテクスチャ、照明と大気 | 実データ描画、日照と視点の検証 | 1200 | not-started |
| 18 | minor-bodies | 小天体の元ファイルと位置評価 | 実データの出典照合と位置のオラクル | 1200 | not-started |
| 19 | spacecraft | 探査機の軌道と経路 | 実ファイルの範囲と補間のオラクル | 1000 | not-started |
| 20 | object-browser | 天体の検索、位置、追従 | 検索・日時の更新・実データによる照合 | 650 | not-started |
| 21 | media | 画像・音声・動画のローカル再生 | 形式の検証、通常入口と配布同梱の確認 | 900 | not-started |
| 22 | parity | 未移植の操作と表示 | 旧版で使用可能な機能を機能ID単位で照合 | 機能ごとに分割 | not-started |

差分が複数のレビュー理由を持つ場合はその単位を分割する。親が変わる場合は先に表を修正し、レビュー指摘は責務を持つ親で直す。CI結果・実機結果・未実装項目を区別する。下位PRから順にレビューし、全機能の実装と検証が揃った時点でマージ作業へ引き継ぐ。
