# Rust移植のレビュー順序

このチャットで確定した「mitaka-plus-rs を新設し、3 OS対応のRust版を実装する」を採用する。移植基準は mocho271828/Mitaka_plus の f9fe62485ad45a3b5651e050a49871a71b6f351d。計画は同一セッションにあり、元ファイルはない。対象、数値・データ・Luaの契約、対象環境、各段階の検証方法を確認した。Gitのバックエンドは vanilla、基点は main。

各ブランチは直前の行を親とする。単位ごとの検証だけで記載した判定ができるため、独立レビュー可とした。先頭6件を submitted-now、それ以降は local-only とする。状態とPRは実績に応じて記録する。

| 順 | ブランチ（codex/ 以下） | レビュー理由・対象 | 検証と独立判定 | 目安 | 状態 |
|---|---|---|---|---|---|
| 1 | foundation | infra・独立ビルド、出典、方針、3 OS CI | 新規クローンのCargoビルドとテストを再現 | 250行 | not-started |
| 2 | catalogs | domain・HIPと星座の原行・破損検出 | 合成データ、実hip.datの件数 | 600行 | not-started |
| 3 | astronomy | domain・日時、座標、惑星暦と観測 | 固定JDの位置、暦表の端、天頂・地平線の試験 | 1400行 | not-started |
| 4 | script-runtime | adapter・固定Luaを実行する別プロセス | 原本のハッシュとローカルスクリプトの実行・失敗 | 650行＋Lua原本 | not-started |
| 5 | sky-desktop | ui・共通の星図、観測地点、操作 | 実データ、投影・対象選択、Macでの起動 | 1300行 | not-started |
| 6 | distribution | infra・各OS用の配布物 | ZIPとdebの内容、展開後の起動、許諾 | 400行 | not-started |
| 7 | solar-scene | domain/ui・太陽系、飛行、地形、大気、食と銀河 | 固定JDでの天体位置、火星への着陸、表示深度 | 機能単位に分割 | not-started |
| 8 | desktop-parity | adapter・JSON v9、各OSの媒体再生、HTML信頼とLuaホストAPI、他の現行対応機能 | 移植元対応表とOS別の操作試験 | 対応表単位に分割 | not-started |

差分が複数のレビュー理由を持つ場合はその単位を分割する。親が変わる場合は先に表を修正し、レビュー指摘は責務を持つ親で直す。CI結果・実機結果・未実装項目を区別する。下位PRから順にレビューし、全機能の実装と検証が揃った時点でマージ作業へ引き継ぐ。
