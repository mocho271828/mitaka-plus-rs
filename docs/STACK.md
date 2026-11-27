# Rust移植のレビュー順序

このチャットで確定した「mitaka-plus-rs を新設し、3 OS対応のRust版を実装する」を採用する。移植基準は mocho271828/Mitaka_plus の f9fe62485ad45a3b5651e050a49871a71b6f351d。計画は同一セッションにあり、元ファイルはない。対象、数値・データ・Luaの契約、対象環境、各段階の検証方法を確認した。Gitのバックエンドは vanilla、基点は main。

各ブランチは直前の行を親とする。単位ごとの検証だけで記載した判定ができるため、独立レビュー可とした。`foundation` から `distribution` までの6件は `main` に統合済み。7番の `lunar` は `main` を親とし、以降は直前の行を親として52件を個別にレビューする。

| 順 | ブランチ（codex/ 以下） | レビュー理由・対象 | 検証と独立判定 | 目安 | 状態 |
|---|---|---|---|---|---|
| 1 | foundation | infra・独立ビルド、出典、方針、3 OS CI | 新規クローンのCargoビルドとテストを再現 | 250行 | [merged #1](https://github.com/mocho271828/mitaka-plus-rs/pull/1) |
| 2 | catalogs | domain・HIPと星座の原行・破損検出 | 合成データ、実hip.datの件数 | 600行 | [merged #2](https://github.com/mocho271828/mitaka-plus-rs/pull/2) |
| 3 | astronomy | domain・日時、座標、惑星暦と観測 | 固定JDの位置、暦表の端、天頂・地平線の試験 | 1400行 | [merged #3](https://github.com/mocho271828/mitaka-plus-rs/pull/3) |
| 4 | script-runtime | adapter・固定Luaを実行する別プロセス | 原本のハッシュとローカルスクリプトの実行・失敗 | 650行＋Lua原本 | [merged #4](https://github.com/mocho271828/mitaka-plus-rs/pull/4) |
| 5 | sky-desktop | ui・共通の星図、観測地点、操作 | 実データ、投影・対象選択、Macでの起動 | 1300行 | [merged #5](https://github.com/mocho271828/mitaka-plus-rs/pull/5) |
| 6 | distribution | infra・各OS用の配布物 | ZIPとdebの内容、展開後の起動、許諾 | 400行 | [merged #6](https://github.com/mocho271828/mitaka-plus-rs/pull/6) |
| 7 | lunar | domain・月の地心位置と地月重心の補正 | 原版由来の位置・姿勢とCPU別丸め誤差を検証 | 700行 | [submitted #7](https://github.com/mocho271828/mitaka-plus-rs/pull/7) |
| 8 | terrain | domain・地形のバイナリ索引と標高 | 切断や負標高の合成試験、実データの火星標高 | 500行 | [submitted #8](https://github.com/mocho271828/mitaka-plus-rs/pull/8) |
| 9 | solar-scene | ui・太陽系と地形・視点移動 | 実データの地球・月を含む11天体と火星地形、飛行・画像出力 | 950行 | [submitted #9](https://github.com/mocho271828/mitaka-plus-rs/pull/9) |
| 10 | galaxy-catalogs | domain・球状星団・銀河・恒星名のカタログ | 固定幅/件数・原バイトの合成試験と実データ | 850行 | [submitted #10](https://github.com/mocho271828/mitaka-plus-rs/pull/10) |
| 11 | deep-sky | ui・銀河の探索と深宇宙表示 | 検索・選択と実画素での描画を検証 | 800行 | [submitted #11](https://github.com/mocho271828/mitaka-plus-rs/pull/11) |
| 12 | satellites | domain・衛星データと参照座標 | 衛星カタログの親子関係と向き・軌道を既存値で検証 | 1400行 | [submitted #12](https://github.com/mocho271828/mitaka-plus-rs/pull/12) |
| 13 | session-v9 | contract・セッション読込・保存 | 旧版で生成したv9を往復し範囲外や不完全な復元を拒否 | 800行 | [submitted #13](https://github.com/mocho271828/mitaka-plus-rs/pull/13) |
| 14 | local-scripting | adapter・GUIからのLuaとローカルHTML操作 | 手動操作・停止・状態適用・信頼の解除 | 1000行 | [submitted #14](https://github.com/mocho271828/mitaka-plus-rs/pull/14) |
| 15 | surface-optics | 天体の自転、食・大気の光学計算 | 原版の数値と光度・遮蔽の境界、実データ | 1800 | [submitted #15](https://github.com/mocho271828/mitaka-plus-rs/pull/15) |
| 16 | playback | 日時の再生・逆再生とカレンダー | 境界停止、月末の扱いとLua経由の時刻設定 | 450 | [submitted #16](https://github.com/mocho271828/mitaka-plus-rs/pull/16) |
| 17 | planet-surfaces | 惑星表面のテクスチャ、照明と大気 | 実データ描画、日照と視点の検証 | 1200 | [submitted #17](https://github.com/mocho271828/mitaka-plus-rs/pull/17) |
| 18 | minor-bodies | 小天体の元ファイルと位置評価 | 実データの出典照合と位置のオラクル | 1200 | [submitted #18](https://github.com/mocho271828/mitaka-plus-rs/pull/18) |
| 19 | media | 画像・音声・動画のローカル再生 | 形式の検証、通常入口と配布同梱の確認 | 900 | [submitted #19](https://github.com/mocho271828/mitaka-plus-rs/pull/19) |
| 20 | spacecraft | 探査機の軌道と経路 | 実ファイルの範囲と補間のオラクル | 1000 | [submitted #20](https://github.com/mocho271828/mitaka-plus-rs/pull/20) |
| 21 | object-browser | 天体の検索、位置、追従 | 検索・日時の更新・実データによる照合 | 650 | [submitted #21](https://github.com/mocho271828/mitaka-plus-rs/pull/21) |
| 22 | satellite-positions | domain・日心衛星位置と冥王星系の重心補正 | 英日定義とC++基準値、ブラウザの位置契約 | 500 | [submitted #22](https://github.com/mocho271828/mitaka-plus-rs/pull/22) |
| 23 | desktop-controls | ui・スクリプトコンソールとデスクトップ操作 | 実Luaの出力、履歴、取消、全画面と画像出力 | 650 | [submitted #23](https://github.com/mocho271828/mitaka-plus-rs/pull/23) |
| 24 | object-orbits | ui・探査機の点・軌跡と選択 | 同時刻標本と投影、旧描画の比較 | 500 | [submitted #24](https://github.com/mocho271828/mitaka-plus-rs/pull/24) |
| 25 | session-selections | contract・セッションの天体選択、保存した設定と復帰 | カタログの実指紋と旧版の再読込を検証 | 500 | [submitted #25](https://github.com/mocho271828/mitaka-plus-rs/pull/25) |
| 26 | projection-window | ui・別ウィンドウへの星図投影 | 同じ描画命令を2画面に表示し、投影の暗転とポインターを検証 | 350 | [submitted #26](https://github.com/mocho271828/mitaka-plus-rs/pull/26) |
| 27 | display-layers | ui・座標グリッド、恒星・星座と観測地点 | 実カタログの識別と合成投影、描画を照合 | 950 | [submitted #27](https://github.com/mocho271828/mitaka-plus-rs/pull/27) |
| 28 | landscape | ui・地平線の地点別パノラマ | 読取専用原画像と方位・仰角を照合 | 500 | [submitted #28](https://github.com/mocho271828/mitaka-plus-rs/pull/28) |
| 29 | galaxy-radiative | domain/ui・銀河輻射と塵吸収の表示 | 星間空間の積分を旧C++と実マップで比較 | 900 | [submitted #29](https://github.com/mocho271828/mitaka-plus-rs/pull/29) |
| 30 | portable-verification | infra・配布物のオフライン起動と3 OS検証 | 実行ファイルから合成カタログを読み、Linux両表示系で描画 | 450 | [submitted #30](https://github.com/mocho271828/mitaka-plus-rs/pull/30) |
| 31 | planet-layers | ui・地心惑星の選択と表示レイヤー | 同時刻の赤経赤緯、模式半径、自転軸、選択を照合 | 650 | [submitted #31](https://github.com/mocho271828/mitaka-plus-rs/pull/31) |
| 32 | camera-controls | ui・カメラの操作とLua | 手動操作と時刻補間、投影と復元を検証 | 900 | [submitted #32](https://github.com/mocho271828/mitaka-plus-rs/pull/32) |
| 33 | terrain-display | ui・地形タイルと倍率 | 元標高と地形倍率を旧版で照合 | 650 | [submitted #33](https://github.com/mocho271828/mitaka-plus-rs/pull/33) |
| 34 | script-calendar | adapter・旧LuaのUTC・地方時と日時補間 | 日時の繰越、時差・実Luaの往復、補間終端 | 450 | [submitted #34](https://github.com/mocho271828/mitaka-plus-rs/pull/34) |
| 35 | registered-media | adapter・登録メディアと制御 | 旧登録名と順序、ローカルメディアを比較 | 900 | [submitted #35](https://github.com/mocho271828/mitaka-plus-rs/pull/35) |
| 36 | milky-way | ui・全天・地平図の銀河背景 | 元の写真・銀河座標と遮蔽、表示設定を検証 | 450 | [submitted #36](https://github.com/mocho271828/mitaka-plus-rs/pull/36) |
| 37 | camera-targets | ui・名前で指定した銀河・球状星団・星座の追従 | 登録名と物理位置の対照、同名の拒否 | 400 | [submitted #37](https://github.com/mocho271828/mitaka-plus-rs/pull/37) |
| 38 | viewer-controls | ui・ポインター選択、距離目盛と表示設定 | Lua・手動変更とセッションの整合 | 500 | [submitted #38](https://github.com/mocho271828/mitaka-plus-rs/pull/38) |
| 39 | sky-deep-catalogs | ui・全天図のNGC・SDSSと地平姿勢 | 全銀河の密度積算と投影座標の対照 | 600 | [submitted #39](https://github.com/mocho271828/mitaka-plus-rs/pull/39) |
| 40 | eclipse-layers | ui・食の境界、本影と半影の円錐 | 旧版の遮蔽物と接線位置の照合 | 550 | [submitted #40](https://github.com/mocho271828/mitaka-plus-rs/pull/40) |
| 41 | display-script | adapter・恒星名、星座指定と惑星の表示倍率 | 実Luaとv9の相互運用、異常入力 | 550 | [submitted #41](https://github.com/mocho271828/mitaka-plus-rs/pull/41) |
| 42 | object-groups | ui・小天体・衛星・探査機の同時表示と軌道 | 分類・日時・表示状態とLuaの往復を照合 | 700 | [submitted #42](https://github.com/mocho271828/mitaka-plus-rs/pull/42) |
| 43 | surface-targets | ui・月と9天体の指定地点への飛行 | 体固定座標とv9着陸地点を旧版で照合 | 550 | [submitted #43](https://github.com/mocho271828/mitaka-plus-rs/pull/43) |
| 44 | horizon-objects | ui・小天体・探査機の地表視差と注視 | 風景の透過度、選択、旧観測方向を比較 | 500 | [submitted #44](https://github.com/mocho271828/mitaka-plus-rs/pull/44) |
| 45 | reproducible-player | infra・再生用WasmのOS間再生成 | Windows改行とmacOS・Linuxの生成バイトを照合 | 150 | [submitted #45](https://github.com/mocho271828/mitaka-plus-rs/pull/45) |
| 46 | interplanetary | ui・天体間の飛行 | 旧版の上昇・巡航・下降と衝突中断を比較 | 550 | [submitted #46](https://github.com/mocho271828/mitaka-plus-rs/pull/46) |
| 47 | shared-galaxy | ui・太陽系から銀河への共通視点 | SDSS分類、全件選択、v9復元を照合 | 650 | [submitted #47](https://github.com/mocho271828/mitaka-plus-rs/pull/47) |
| 48 | physical-sky | ui・宇宙・地表の恒星点群 | 同じ視点からの地形・星・星座と遮蔽を比較 | 700 | [submitted #48](https://github.com/mocho271828/mitaka-plus-rs/pull/48) |
| 49 | static-html | adapter・信頼したローカルHTMLの静的レイアウト | 表・CSS・リンクをローカル実表示し遷移と再照合を検証 | 450 | [submitted #49](https://github.com/mocho271828/mitaka-plus-rs/pull/49) |
| 50 | physical-terrain | ui・地表メッシュを物理カメラに統合 | 地形の交点深度、光源、視点に対する稜線の照合 | 550 | [submitted #50](https://github.com/mocho271828/mitaka-plus-rs/pull/50) |
| 51 | horizon-transition | ui・地平図の離着陸と投影の引き継ぎ | 地上の姿勢と視野、切り替え開始・終了を照合 | 500 | [submitted #51](https://github.com/mocho271828/mitaka-plus-rs/pull/51) |
| 52 | package-player | infra・各配布物のLuaと音声再生 | ネイティブデコーダーの再生・停止・シークを3 OSで検証 | 200 | [submitted #52](https://github.com/mocho271828/mitaka-plus-rs/pull/52) |
| 53 | object-occlusion | ui・軌道・探査機の地形遮蔽 | 旧描画順と地表でのクリック対象を照合 | 300 | [submitted #53](https://github.com/mocho271828/mitaka-plus-rs/pull/53) |
| 54 | terrain-revision | ui・飛行中の起伏倍率変更 | 高度と到着時刻を保ち地表からの距離を再評価 | 200 | [submitted #54](https://github.com/mocho271828/mitaka-plus-rs/pull/54) |
| 55 | native-shortcuts | ui・従来のキー操作 | メニューとコマンドから同じ標準操作を実行 | 200 | [submitted #55](https://github.com/mocho271828/mitaka-plus-rs/pull/55) |
| 56 | solar-background | ui・物理視点での天の川全天写真 | 恒星・銀河の背後からの投影とpc距離による減光を比較 | 350 | [submitted #56](https://github.com/mocho271828/mitaka-plus-rs/pull/56) |
| 57 | fullscreen-state | ui・全画面の遷移中の操作 | OSの通知と反対方向の二重要求、期限後の再試行を検証 | 200 | [submitted #57](https://github.com/mocho271828/mitaka-plus-rs/pull/57) |
| 58 | parity | contract・旧版機能と三OS配布の検証 | 機能別の対応実績、配布物と起動経路を検証 | 機能ごとに分割 | [submitted #58](https://github.com/mocho271828/mitaka-plus-rs/pull/58) |

差分が複数のレビュー理由を持つ場合はその単位を分割する。親が変わる場合は先に表を修正し、レビュー指摘は責務を持つ親で直す。CI結果・実機結果・未実装項目を区別する。下位PRから順にレビューし、全機能の実装と検証が揃った時点でマージ作業へ引き継ぐ。
