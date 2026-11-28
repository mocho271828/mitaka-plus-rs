# 旧macOS版との差分解消計画 第2期

対象はマージ後の `main@b69c4c4` と旧macOS版 `f9fe624` の再照合で確認した24件。最初に、`Time:setJDTo` の後も時刻再生が続くこと、同じデータの再読込で表示選択が保たれることを直す。高6件・中13件・低5件の順に進める。旧Windows版固有の20件とPro専用の3件は対象外。[Rustの基準コミット](https://github.com/mocho271828/mitaka-plus-rs/tree/b69c4c46189d73a769e3661f02f408c4dad924a7)・[旧macOS版の基準コミット](https://github.com/mocho271828/Mitaka_plus/tree/f9fe62485ad45a3b5651e050a49871a71b6f351d)・[移植範囲](COMPATIBILITY.md)

前期 #59〜#78 の月末の月送り、全HIP検索、起動時の表示設定復元、配布物からのGUI起動などは基準コミットに含まれる。月送りの既存回帰条件は `2024-01-31 23:59:59.75 UTC` → `2024-03-02 23:59:59.75 UTC`。本計画では前期の完了項目を守りながら、別の入口・データ構成で残った差を扱う。[前期計画と提出記録](MACOS_PARITY_PLAN.md)

## `/vibe-stacking` への引き継ぎ

- 次回の入力は `/vibe-stacking docs/MACOS_PARITY_PLAN_2.md`。現在は計画のみで、全24単位の実装状態は `not-started`。各単位に変更箇所、固定した旧版ソース、独立して試せる合格条件を記載した。
- 検出したバックエンドは `vanilla`。予定ブランチは各見出しの短い名前に `codex/parity2-` を付ける。01の親は `main`、02〜24は一つ前の番号のブランチ。番号は提出順でもあり、各単位は親との差分でレビューできる。実装開始時に `main` が進んでいたら、各項目の再現と既存修正を確認する。
- 次回実装時の初回提出枠は高優先の01〜06を `submitted-now`、07〜24を `local-only` とする。以降は親から最大6件ずつ繰り上げる。将来のPRタイトルには `(Stacked PR)` を付ける。現時点の提出件数は0件。
- 以下の「合格条件」をその単位の独立レビュー条件とする。差分目安は試験を含む追加・削除行の合計。内容の重なる箇所では11がビューの日時表示、15が投影先への合成、13が選択操作、17が情報表示を担当する。指摘は該当単位で修正し、分割・統合が必要なら元の番号と合格条件を保って親と提出範囲を更新する。
- 自動試験の入力には合成データを使い、各単位で通常の操作入口も1回以上通す。下記の具体的な天体・時刻・表示件数は固定コミットの比較値。実データや旧パネルは利用できる開発機で読み取り専用の照合に使い、通常CIには持ち込まない。[実装方針](../IMPLEMENTATION_POLICY.md)・[データと出典のgate](../policy_gates/data.md)

## 優先度 高（01〜06）

### 01　Luaの日時補間後も時刻再生を続ける `time-animation`

- **実装範囲**（domain、90–180行程度）　`Time:setJDTo` が再生を止める経路を修正する。補間中のJDを作動中の再生時計に反映し、通常どおり到達して進行方向に暦表の余地があればその地点から元の速度で続ける。データ切替時の取消と時計中断時の停止を保つ。
- **合格条件**　順・逆（速度 +2、−2）で4日先へ補間し、途中の `Time.speed`、到達JD、補間解除後の通常の再生更新でJDが進む向きを照合する。速度2から `Time:setJDTo(2451549,0.1)` を実行して到達後も再生し、停止中からの補間は停止のまま終わる。補間中の `Time.speed=0` は再生だけを止める。GUIの通常再生でも到達後に元の倍率で進む。
- **対象と根拠**（旧版制御試験・Rustホスト実測）　[playback.rs](../crates/mitaka-desktop/src/playback.rs)・[script_ui/mod.rs](../crates/mitaka-desktop/src/script_ui/mod.rs)・[旧版 PlaybackController.mm](https://github.com/mocho271828/Mitaka_plus/blob/f9fe62485ad45a3b5651e050a49871a71b6f351d/vendor/mitaka/native/gui/src/PlaybackController.mm#L190)・[旧版の順逆再生試験](https://github.com/mocho271828/Mitaka_plus/blob/f9fe62485ad45a3b5651e050a49871a71b6f351d/vendor/mitaka/native/gui/tests/test_time_animation_controller.mm#L176)

### 02　同じデータの再読込で表示選択を維持する `reload-selection`

- **実装範囲**（adapter、180–320行程度）　読込成功時に恒星名・星座・小天体・衛星の選択と惑星軌道表示を新しいモデルに結び直す。共通HIPは選択・非選択を保ち、新規HIPは新表の既定、消失HIPと消失した星座略号は除外する。小天体と衛星の表示集合は出典キー・元ファイル内容・原行で照合する。
- **合格条件**　恒星名0件・非表示の星座1件・惑星軌道オフで同じフォルダを開き直し、3値が同じことを確認する。小天体・衛星の出典と原行が一致する場合と変わった場合、読込失敗と連続要求でも、誤った選択や古い読込結果を反映しない。初回起動の自動復元と読込開始時のLua・時刻再生停止は既存試験で守る。
- **対象と根拠**（両版実測）　[data_loading.rs](../crates/mitaka-desktop/src/data_loading.rs)・[layers/state.rs](../crates/mitaka-desktop/src/layers/state.rs)・[objects/selections.rs](../crates/mitaka-desktop/src/objects/selections.rs)・[旧版 AppController.mm](https://github.com/mocho271828/Mitaka_plus/blob/f9fe62485ad45a3b5651e050a49871a71b6f351d/vendor/mitaka/native/gui/src/AppController.mm#L345)・[旧版の恒星名再照合](https://github.com/mocho271828/Mitaka_plus/blob/f9fe62485ad45a3b5651e050a49871a71b6f351d/vendor/mitaka/native/gui/src/star_name_selection.cpp#L28)・[旧版の衛星再照合](https://github.com/mocho271828/Mitaka_plus/blob/f9fe62485ad45a3b5651e050a49871a71b6f351d/vendor/mitaka/native/gui/src/satellite_display.cpp#L73)

### 03　英語・日本語の恒星名表から検索・v9復元を可能にする `star-name-tables`

- **実装範囲**（adapter、120–240行程度）　GUIの恒星名の取得元を表示・ブラウザ・復元で揃える。旧GUIは `star_names_jp.dat` を優先し、そのファイルが存在しないときに `star_names.dat` を使う。両方がない場合は現行の `star_names_HIP.dat` 対応を保つ。Luaのカメラ対象名は専用のHIP表を使う。
- **合格条件**　日本語表のみ／英語表のみで原綴り `Sirius`（HIP 32349）を検索し、旧版互換v9の選択を復元する。基準実データでは各174件。3表共存・HIP表のみ・全表なし・保存HIP消失も試し、未知のHIPで部分復元しない。
- **対象と根拠**（両版実測）　[layers/catalog.rs](../crates/mitaka-desktop/src/layers/catalog.rs)・[objects/catalog.rs](../crates/mitaka-desktop/src/objects/catalog.rs)・[layers/state.rs](../crates/mitaka-desktop/src/layers/state.rs)・[crates/mitaka-core/src/extended_catalog.rs](../crates/mitaka-core/src/extended_catalog.rs)・[旧版 extended_catalogs.cpp](https://github.com/mocho271828/Mitaka_plus/blob/f9fe62485ad45a3b5651e050a49871a71b6f351d/vendor/mitaka/native/gui/src/extended_catalogs.cpp#L42)

### 04　飛行中のLua距離指定をカメラに反映する `flight-zoom`

- **実装範囲**（domain、120–240行程度）　候補距離を旧版のカメラ範囲で検証してから航行を中断し、現在の視点と姿勢を保って即時・補間ズームへ移る。旧飛行の更新で距離を書き戻さない。
- **合格条件**　月面から3秒で離陸し50 ms後に `Camera:setZoomTo(2*UNIT_AU,0)`。直後と0.3秒後は2 AU。軌道カメラで2 AUから `1e-11 AU` はLuaエラーで元の距離を保持。時間付きズームの終了、停止中の月面での拒否も確かめる。
- **対象と根拠**（両版実測）　[camera_control/mod.rs](../crates/mitaka-desktop/src/camera_control/mod.rs)・[camera_control/navigation.rs](../crates/mitaka-desktop/src/camera_control/navigation.rs)・[solar.rs](../crates/mitaka-desktop/src/solar.rs)・[旧版 SolarSystemView.mm](https://github.com/mocho271828/Mitaka_plus/blob/f9fe62485ad45a3b5651e050a49871a71b6f351d/vendor/mitaka/native/gui/src/SolarSystemView.mm#L616)

### 05　太陽系パネルの近接表示で手動ズーム後も惑星を追従する `planet-tracking`

- **実装範囲**（domain、60–140行程度）　太陽系パネルの単一天体の近接表示で、ズーム時に追従対象を維持し、日時更新時はその天体の新しい位置を使う。
- **合格条件**　火星をパネルから近接表示→ピンチ→JD 2451545から2451546へ進める。中心のy座標が前日の−0.013416 AUに残らず翌日の火星の+0.001771 AUと一致し、手動の距離を保つ。ブラウザ経由と地球・月の中点追従も確認する。
- **対象と根拠**（両版実測）　[solar.rs](../crates/mitaka-desktop/src/solar.rs)・[camera_control/target.rs](../crates/mitaka-desktop/src/camera_control/target.rs)・[旧版 SolarSystemView.mm](https://github.com/mocho271828/Mitaka_plus/blob/f9fe62485ad45a3b5651e050a49871a71b6f351d/vendor/mitaka/native/gui/src/SolarSystemView.mm#L652)

### 06　着陸後の手動ズームと水平ドラッグを旧版に合わせる `surface-input`

- **実装範囲**（ui、80–180行程度）　地表の手動入力を高度と方位へ反映する。地表専用の処理を、軌道視点のズームやLuaの画角指定と区別する。
- **合格条件**　月面・高度2 m・視野45°で2倍相当の縮小を入力し、高度4 m・視野45°になる。北向きから右に20 ptのドラッグで方位353°。逆方向と高度下限も比較用イベントで試す。物理ジェスチャーの確認は別表。
- **対象と根拠**（入力イベントで両版実測）　[solar.rs](../crates/mitaka-desktop/src/solar.rs)・[surface.rs](../crates/mitaka-desktop/src/surface.rs)・[main.rs](../crates/mitaka-desktop/src/main.rs)・[旧版 SolarSystemNavigation.mm](https://github.com/mocho271828/Mitaka_plus/blob/f9fe62485ad45a3b5651e050a49871a71b6f351d/vendor/mitaka/native/gui/src/SolarSystemNavigation.mm#L425)

## 優先度 中（07〜19）

### 07　GUI逆再生の向きをv9に保存する `signed-rate`

- **実装範囲**（contract、60–130行程度）　通常再生の向きを符号付き `simulation_rate` に反映して保存する。v9の形式とLuaの `plus_time.speed` は別々に扱う。
- **合格条件**　倍率3600で順再生と逆再生を行い、JDの推移と保存値 +3600／−3600 を照合する。倍率変更で向きを維持し、復元直後は通常倍率の符号を保った停止状態、Luaの再生速度は0になる。
- **対象と根拠**（両版実測）　[playback.rs](../crates/mitaka-desktop/src/playback.rs)・[session_ui.rs](../crates/mitaka-desktop/src/session_ui.rs)・[session.rs](../crates/mitaka-desktop/src/session.rs)・[旧版 PlaybackController.mm](https://github.com/mocho271828/Mitaka_plus/blob/f9fe62485ad45a3b5651e050a49871a71b6f351d/vendor/mitaka/native/gui/src/PlaybackController.mm#L369)

### 08　画像だけのリンクから旧HTMLパネルを操作する `html-image-links`

- **実装範囲**（ui、100–220行程度）　画像を囲むアンカーと配置後の矩形を結び、既存の文字リンクと同じ文書単位の実行許可・ホスト処理を通す。操作ガイドの画像リンクの制限表記を合わせる。
- **合格条件**　信頼済みの日食パネルで先頭画像の中心と直下の文字を個別にクリックし、各回 `dofile('ui/scripts/eclipse_20090722.lua');` が1回だけ届く。合成文書で画像外・許可のない文書も検査。旧版との比較対象は指定寸法のリンク領域。
- **対象と根拠**（両版でクリック実測）　[script_ui/document/layout.rs](../crates/mitaka-desktop/src/script_ui/document/layout.rs)・[script_ui/document/draw.rs](../crates/mitaka-desktop/src/script_ui/document/draw.rs)・[script_ui/document.rs](../crates/mitaka-desktop/src/script_ui/document.rs)・[docs/DISPLAY.md](../docs/DISPLAY.md)・[旧版 HtmlBrowserController.mm](https://github.com/mocho271828/Mitaka_plus/blob/f9fe62485ad45a3b5651e050a49871a71b6f351d/vendor/mitaka/native/gui/src/HtmlBrowserController.mm#L13)

### 09　地表と表面観察で惑星・月の軌道線を隠す `surface-orbits`

- **実装範囲**（domain、45–110行程度）　旧版の地表・表面観察プレビューの条件で、惑星と月の軌道の描画を抑える。表示設定を保ち、離陸後の宇宙視点とドラッグ中の簡易描画を区別する。
- **合格条件**　月面2 m、方位60°、仰角0°、JD 2451545、480×320で、現在の135線分（地平線より上68本）が0本になる。宇宙へ戻ると設定どおり再表示し、地球・月の接近表示では従来どおり月軌道だけが表示される。
- **対象と根拠**（Rust描画実測・旧ソース照合）　[solar.rs](../crates/mitaka-desktop/src/solar.rs)・[solar/surface_flight.rs](../crates/mitaka-desktop/src/solar/surface_flight.rs)・[旧版 SolarSceneRenderer.mm](https://github.com/mocho271828/Mitaka_plus/blob/f9fe62485ad45a3b5651e050a49871a71b6f351d/vendor/mitaka/native/gui/src/SolarSceneRenderer.mm#L117)

### 10　視点の手前を横切る軌道線の可視部分を描く `orbit-clipping`

- **実装範囲**（domain、60–160行程度）　惑星・月の軌道の各線分に既存の `SolarCamera::project_segment` を使い、表示倍率と位置変換後に手前の面と画面端で切り詰める。標本自体は既存のものを使う。
- **合格条件**　800×600、JD 2451546、月の近接表示、yaw 330°、pitch 0°、距離0.0000697105644 AUで、地球軌道標本90→91を (800, 395.976) から (0, 323.662) まで約803 px描く（端点差1 px以内）。両端が前・後、片端だけが後、逆順も検査。
- **対象と根拠**（両版の描画経路で実測）　[solar.rs](../crates/mitaka-desktop/src/solar.rs)・[solar/scales.rs](../crates/mitaka-desktop/src/solar/scales.rs)・[旧版 solar_camera.cpp](https://github.com/mocho271828/Mitaka_plus/blob/f9fe62485ad45a3b5651e050a49871a71b6f351d/vendor/mitaka/native/gui/src/solar_camera.cpp#L297)

### 11　固定時差と小数秒による暦入力と主画面の日時表示を揃える `local-calendar`

- **実装範囲**（ui、120–260行程度）　カレンダーの時分秒・小数秒を選択中の固定時差でJDへ変換する。主窓の常設日付欄はJDと設定時差での年月日・時分、地平図・太陽系内の時刻表示はJDとする。既存のUTC・JD直接入力も使えるようにする。
- **合格条件**　UTC+09:00 の `2024-01-02 03:00:00.75` がJD 2460311.25000868（許容差1e−9日）になる。負の時差・日付またぎ・範囲外のJD保持、表示のオン・オフを確認。GUIの東正の時差とLuaの西正の時差を独立に試す。
- **対象と根拠**（入力は実測・表示はソース照合）　[main.rs](../crates/mitaka-desktop/src/main.rs)・[playback.rs](../crates/mitaka-desktop/src/playback.rs)・[settings_ui.rs](../crates/mitaka-desktop/src/settings_ui.rs)・[crates/mitaka-core/src/astronomy.rs](../crates/mitaka-core/src/astronomy.rs)・[旧版 AppTimeCommands.mm](https://github.com/mocho271828/Mitaka_plus/blob/f9fe62485ad45a3b5651e050a49871a71b6f351d/vendor/mitaka/native/gui/src/AppTimeCommands.mm#L329)

### 12　暦表の端で範囲外に向かうLua再生を即時に拒否する `playback-boundary`

- **実装範囲**（domain、55–130行程度）　`Time.speed` の受付時に、現在のJDとステップから求めた実効速度の方向を検査する。最初の描画より前に呼び出し元へ結果を返す。
- **合格条件**　最終JD 2487980.5で外向きの `Time.speed=1` はLuaのエラーになり、停止中なら速度0・同じJDを保つ。上限からの逆方向と下限からの順方向は受理し、下限からの外向きは拒否する。
- **対象と根拠**（両版実測）　[playback.rs](../crates/mitaka-desktop/src/playback.rs)・[script_ui/mod.rs](../crates/mitaka-desktop/src/script_ui/mod.rs)・[旧版 playback_clock.cpp](https://github.com/mocho271828/Mitaka_plus/blob/f9fe62485ad45a3b5651e050a49871a71b6f351d/vendor/mitaka/native/gui/src/playback_clock.cpp#L16)

### 13　天体ブラウザで選択と中央表示を選び分ける `browser-select`

- **実装範囲**（ui、100–210行程度）　一般天体ブラウザの行クリックは候補を強調する。「選択」でアプリの対象を更新し、「選択して中央に表示」で対象へ視点を移す。操作時のビューは旧版の天体別規則に従う（恒星・NGC・SDSSは全天図、地球は太陽系）。
- **合格条件**　全天図の中心（赤経180°・赤緯0°）・倍率1からシリウスの行を押すとアプリの対象はそのまま。「選択」で対象だけが変わり、構図を保つ。「選択して中央に表示」で初めて（101.289°、−16.7131°）・倍率4になる。対象切替、無名HIP、惑星の中央表示も確認する。
- **対象と根拠**（両版実測）　[objects/mod.rs](../crates/mitaka-desktop/src/objects/mod.rs)・[objects/app.rs](../crates/mitaka-desktop/src/objects/app.rs)・[objects/search.rs](../crates/mitaka-desktop/src/objects/search.rs)・[旧版 AppPlusCommands.mm](https://github.com/mocho271828/Mitaka_plus/blob/f9fe62485ad45a3b5651e050a49871a71b6f351d/vendor/mitaka/native/gui/src/AppPlusCommands.mm#L145)

### 14　観測地の手動適用で進行中の地点移動を止める `observer-apply`

- **実装範囲**（ui、85–175行程度）　旧版の適用操作に合わせてLuaの地点移動を取り消し、入力を検証する。有効な地球上の地点では観測地を更新し、地平図に切り替えて観測地設定を閉じる。
- **合格条件**　地平図でLua地点移動の途中に手入力地点を適用する。次フレームと元の移動終了時にも手入力座標を保ち、日時設定の開閉状態は独立している。範囲外と地球以外の登録地点は適用できない。
- **対象と根拠**（ソース照合）　[settings_ui.rs](../crates/mitaka-desktop/src/settings_ui.rs)・[camera_control/places.rs](../crates/mitaka-desktop/src/camera_control/places.rs)・[旧版 AppObserverCommands.mm](https://github.com/mocho271828/Mitaka_plus/blob/f9fe62485ad45a3b5651e050a49871a71b6f351d/vendor/mitaka/native/gui/src/AppObserverCommands.mm#L132)

### 15　投影先のポインターと観測JDを投影画面に反映する `projector-overlay`

- **実装範囲**（ui、100–230行程度）　投影ウィンドウ内の移動イベントとその座標でポインターを描く。11番で追加するビュー内JDを含む描画を共有し、主画面の操作UIは含めない。
- **合格条件**　投影先に入力がないときは中央、投影先だけで動かすとその座標に表示し、主画面だけで動かしても位置を保つ。地平図・太陽系で時刻のオン・オフを主画面と投影先の両方で照合し、暗転時も旧版の重なり順を確認する。
- **対象と根拠**（ソース照合）　[projector.rs](../crates/mitaka-desktop/src/projector.rs)・[main.rs](../crates/mitaka-desktop/src/main.rs)・[旧版 DisplayOutputController.mm](https://github.com/mocho271828/Mitaka_plus/blob/f9fe62485ad45a3b5651e050a49871a71b6f351d/vendor/mitaka/native/gui/src/DisplayOutputController.mm#L36)

### 16　画像書き出しは保存先を選ぶ前のフレームを使う `capture-instant`

- **実装範囲**（adapter、90–210行程度）　保存要求時のビュー画像・切出し範囲・倍率を一緒に確保し、その画像を保存先決定後に書き出す。GPUからの取得に失敗した場合と保存ダイアログの取消を個別に扱う。
- **合格条件**　異なる日時の二つのフレームA・Bで、Aで保存を要求しBで保存先を決定しても、内容・矩形はAと一致する。取消時はファイルを作らず、次の要求に画像を持ち越さない。PNG・JPEG・BMPと既存の原本保護を維持する。
- **対象と根拠**（ソース照合）　[main.rs](../crates/mitaka-desktop/src/main.rs)・[capture.rs](../crates/mitaka-desktop/src/capture.rs)・[旧版 AppSessionCommands.mm](https://github.com/mocho271828/Mitaka_plus/blob/f9fe62485ad45a3b5651e050a49871a71b6f351d/vendor/mitaka/native/gui/src/AppSessionCommands.mm#L336)

### 17　選択天体の情報にNGC/IC・SDSSの詳細を出す `deep-sky-info`

- **実装範囲**（ui、90–210行程度）　13番で選んだ天体の種別・識別子を情報パネルにも渡す。既存のカタログ詳細を使い、天体切替・データ切替後に前の選択を表示しない。
- **合格条件**　全天図でNGC/ICを選ぶと名称・赤経・赤緯・等級（V／写真／未記載）・視直径、SDSS銀河とクエーサーでは座標・距離と各々のeClass／zが出る。恒星・惑星と未選択にも切り替えて表示を照合する。
- **対象と根拠**（ソース照合）　[main.rs](../crates/mitaka-desktop/src/main.rs)・[objects/app.rs](../crates/mitaka-desktop/src/objects/app.rs)・[objects/catalog.rs](../crates/mitaka-desktop/src/objects/catalog.rs)・[旧版 SkyView.mm](https://github.com/mocho271828/Mitaka_plus/blob/f9fe62485ad45a3b5651e050a49871a71b6f351d/vendor/mitaka/native/gui/src/SkyView.mm#L333)

### 18　再生準備中とデコーダーが確定した失敗を区別する `media-error`

- **実装範囲**（adapter、110–240行程度）　メディア要素の失敗理由を現行のヘルパー応答から親アプリへ伝える。再生中のファイルと通知を対応付け、再生可能な形式は各OSのデコーダーの判断を使う。
- **合格条件**　破損MP4でデコーダーが失敗を確定したら、親に「再生できません」と理由を表示する。旧版で失敗したWMV2も、利用中のOSでデコード非対応なら照合する。受理するOSには失敗を要求しない。次に正常な素材へ切り替えて再生でき、旧素材の遅い通知で表示が戻らない。
- **対象と根拠**（両版のmacOSで実測）　[crates/mitaka-media-web/src/lib.rs](../crates/mitaka-media-web/src/lib.rs)・[bin/mitaka-media/control.rs](../crates/mitaka-desktop/src/bin/mitaka-media/control.rs)・[media_preview/mod.rs](../crates/mitaka-desktop/src/media_preview/mod.rs)・[旧版 MediaController.mm](https://github.com/mocho271828/Mitaka_plus/blob/f9fe62485ad45a3b5651e050a49871a71b6f351d/vendor/mitaka/native/gui/src/MediaController.mm#L288)

### 19　全天図・地平図の座標格子に目盛と凡例を表示する `grid-labels`

- **実装範囲**（ui、140–280行程度）　ビューと有効な格子に応じて角度・時角の目盛、方位、系統ごとの凡例を旧版の座標で描く。拡大・ロール時に画面外へ出た目盛と既存の方位表示の重複を処理する。
- **合格条件**　標準の800×600で全天図の赤道格子だけなら赤緯15°刻み・赤経2時間刻み・凡例「赤道」で24件。地平図の地平格子だけなら高度・8方位・凡例で20件。黄道だけなら凡例「黄道」、全オフなら目盛・凡例とも0件。
- **対象と根拠**（両版の描画経路で実測）　[layers/grid.rs](../crates/mitaka-desktop/src/layers/grid.rs)・[layers/render.rs](../crates/mitaka-desktop/src/layers/render.rs)・[chart.rs](../crates/mitaka-desktop/src/chart.rs)・[旧版 GridRenderer.mm](https://github.com/mocho271828/Mitaka_plus/blob/f9fe62485ad45a3b5651e050a49871a71b6f351d/vendor/mitaka/native/gui/src/GridRenderer.mm#L174)

## 優先度 低（20〜24）

### 20　Luaコンソールの戻り値を安全に整形して表示する `console-results`

- **実装範囲**（adapter、90–200行程度）　コンソールの戻り値をグローバル `print` に依存せず整形する。表示文字列とLuaの実行成否を分け、UTF-8境界、8,192バイトの本文上限と省略記号をホストまで維持する。
- **合格条件**　`print=nil; return 42` は成功して `=> 42`、`return string.char(255)` は成功して `[non-UTF-8 output]`。9,000個の `a` では `=> `＋8,189個＋`... [truncated]`、本文ちょうど8,192バイトなら接尾辞なし。複数戻り値と実際に失敗する `__tostring` も照合。
- **対象と根拠**（両版実測）　[crates/mitaka-script/src/main.rs](../crates/mitaka-script/src/main.rs)・[crates/mitaka-script/src/host.rs](../crates/mitaka-script/src/host.rs)・[script_ui/mod.rs](../crates/mitaka-desktop/src/script_ui/mod.rs)・[旧版 lua_console.cpp](https://github.com/mocho271828/Mitaka_plus/blob/f9fe62485ad45a3b5651e050a49871a71b6f351d/vendor/mitaka/native/gui/src/lua_console.cpp#L22)

### 21　等級スライダーを読込データの最暗等級まで広げる `magnitude-range`

- **実装範囲**（ui、65–160行程度）　スライダーの上端は9、恒星の最大等級、NGC/ICの最大等級のうち最大の値とする。遅延読込と両立させ、個別天体を選ぶ既存経路とも上限を揃える。
- **合格条件**　合成データで恒星とNGC/ICのどちらが最暗でもその等級を指定できる。実データでは10,213件中16等より暗い9件を範囲に含み、18等のIC 2870に到達する。NGCなしと別データへの切替でも上端を再計算する。
- **対象と根拠**（両ローダーで集計・UIはソース照合）　[settings_ui.rs](../crates/mitaka-desktop/src/settings_ui.rs)・[deep_sky/mod.rs](../crates/mitaka-desktop/src/deep_sky/mod.rs)・[layers/catalog.rs](../crates/mitaka-desktop/src/layers/catalog.rs)・[旧版 AppController.mm](https://github.com/mocho271828/Mitaka_plus/blob/f9fe62485ad45a3b5651e050a49871a71b6f351d/vendor/mitaka/native/gui/src/AppController.mm#L347)

### 22　フォルダ全体を開き直さずローカル地形を再読込する `reload-terrain`

- **実装範囲**（adapter、90–200行程度）　「ローカル地形を再読込」から現在のデータルートの地形・着陸標高に関わるキャッシュと読込失敗の記録を破棄し、再描画する。
- **合格条件**　合成地形なしで表示→地形を追加→再読込→高さを変えて再読込し、各回の標高と描画が新しい内容を反映する。削除時は基準球面に戻り、操作を通してJD・選択天体を保つ。
- **対象と根拠**（ソース照合）　[commands.rs](../crates/mitaka-desktop/src/commands.rs)・[solar/sphere.rs](../crates/mitaka-desktop/src/solar/sphere.rs)・[solar/terrain.rs](../crates/mitaka-desktop/src/solar/terrain.rs)・[main.rs](../crates/mitaka-desktop/src/main.rs)・[旧版 SolarTerrain.mm](https://github.com/mocho271828/Mitaka_plus/blob/f9fe62485ad45a3b5651e050a49871a71b6f351d/vendor/mitaka/native/gui/src/SolarTerrain.mm#L44)

### 23　選択中のビューの「描画の情報」を開けるようにする `render-info`

- **実装範囲**（ui、70–160行程度）　操作メニューから現在のビューが使う座標系・描画診断を表示する。旧版の各ビューの説明を比較元とし、詳細がない場合は既存の状態表示を使う。
- **合格条件**　同じデータで全天図・地平図・太陽系を順に選び、各ビューに対応する説明が出ることを確認する。「選択天体の情報」と別の操作として使え、説明がない場合の表示も確認する。
- **対象と根拠**（ソース照合）　[commands.rs](../crates/mitaka-desktop/src/commands.rs)・[main.rs](../crates/mitaka-desktop/src/main.rs)・[chart.rs](../crates/mitaka-desktop/src/chart.rs)・[solar.rs](../crates/mitaka-desktop/src/solar.rs)・[旧版 AppWindow.mm](https://github.com/mocho271828/Mitaka_plus/blob/f9fe62485ad45a3b5651e050a49871a71b6f351d/vendor/mitaka/native/gui/src/AppWindow.mm#L130)

### 24　v9読込時にPlus時刻ステップの速度換算を検証する `session-rate-overflow`

- **実装範囲**（contract、40–110行程度）　v9の `plus_time` を適用する前に `value_days × speed × 86400` の有限性を検証する。旧版と同じ計算順を使い、通常再生の倍率上限とは独立に判定する。
- **合格条件**　カスタムステップ `value_days=1e304, speed=1` はデコードで拒否し、現在のセッションを保つ。同じ日数で速度0なら受理し、有限の正・負の速度を持つ既存の正常v9も読める。
- **対象と根拠**（両版デコーダーで実測）　[session.rs](../crates/mitaka-desktop/src/session.rs)・[session_ui.rs](../crates/mitaka-desktop/src/session_ui.rs)・[playback.rs](../crates/mitaka-desktop/src/playback.rs)・[旧版 session_state.mm](https://github.com/mocho271828/Mitaka_plus/blob/f9fe62485ad45a3b5651e050a49871a71b6f351d/vendor/mitaka/native/gui/src/session_state.mm#L90)

## 実機で未確認の項目

以下は実装24単位と別に追跡する。各行の状態は**未検証**。試験時にはコミット、OS・機種、GPU・表示倍率・入力／音声装置、素材と操作、期待値と実測値を記録する。差が出た場合に、該当する単位または後続調査に結び付ける。

| 対象 | 実機での手順と比較点 | 現在の証拠 |
| --- | --- | --- |
| Windows 11・Intel Mac | 配布物を展開して起動し、データを開き主画面と投影画面を表示する。実GPU、フォント、画面倍率で文字欠け・位置ずれを記録する。 | Apple Siliconでの描画、3 OSのCIでの配布検査、macOSのUniversalバイナリ |
| 複数の物理モニター | 異なる倍率の画面へ独立画像ウィンドウを移し、移動前後の画像と位置を照合する。投影先の四隅・中央でポインターとJDの位置を確認する（15）。 | 独立ウィンドウと投影の描画、ポインター入力元のソース比較 |
| 動画シーク・音声出力 | Windows 11・Ubuntuで固定素材と利用環境のコーデックを試す。動画は45/90フレーム、PCM WAVは48,000/96,000フレームが各3秒。実スピーカーで出音を確認し、再生中の切断・出力先切替の結果を記録する（18）。 | 3 OSのCIでは固定素材のシークを確認。仮想出力を含む |
| 物理トラックパッド | 同じMac・機器で旧版とRust版の太陽系の回転、月面のピンチ、左右ドラッグを比較する。方位・ロール・高度・視野角を前後で記録する（05・06）。 | 比較用入力で月面操作を再現。回転ジェスチャーの旧版の受け渡しとRust依存側の扱いはソースで確認 |
| OSのスリープ・復帰 | Lua待機、飛行、日時補間をそれぞれ開始してスリープ・復帰。停止・継続の状態、復帰後のJD・姿勢と遅延した操作の有無を旧版と照合する（01・04）。 | 旧版にはスリープ通知時の取消処理がある。実際の復帰動作は未測定 |

[検証環境](COMPATIBILITY.md)・[媒体とシークの検証条件](MEDIA.md)・[基準コミットの既存CI](https://github.com/mocho271828/mitaka-plus-rs/actions/runs/46109761279)

## 実装時の検証と完了条件

1. 各単位では上記の現象を再現し、親との差分だけで合格条件を検証する。ソース照合だった項目は初めに操作または描画の再現試験を置く。再生・飛行は制御できる時刻、描画は固定の画面寸法と入力を使う。複数フレームの結果、失敗後の状態も比較する。
2. 該当crateの `cargo test --locked -p mitaka-desktop`、`cargo test --locked -p mitaka-core`、`cargo test --locked -p mitaka-script` を選ぶ。Lua操作は同梱ヘルパーとGUIスモーク、18は実プレーヤーの試験を使う。媒体制御のソース変更時は固定ツールチェーンでJS・Wasmを再生成し `bash tools/media-web.sh --check` を通す。[CIの実行手順](../.github/workflows/ci.yml)
3. スタック先端で `cargo fmt --all -- --check`、`cargo build --locked --workspace`、`cargo clippy --locked --workspace --all-targets -- -D warnings`、`cargo test --locked --workspace` を通す。既存の3 OSの配布GUI、UbuntuのWayland／X11、空のCargoキャッシュからのオフラインビルドの結果を実装コミットに対応付ける。[配布ワークフロー](../.github/workflows/package.yml)
4. 24単位それぞれに親、実測差分、試験条件と結果を記録する。v9は既存フォーマットを使い、移行作業は不要。原本と許諾表示を保全し、各作業単位でGit状態・差分量と3つのpolicy gateの作業コピーを確認する。実機の結果には実施環境を添え、未検証ならその状態を維持する。[開発規則](../AGENTS.md)・[検証gate](../policy_gates/testing.md)
