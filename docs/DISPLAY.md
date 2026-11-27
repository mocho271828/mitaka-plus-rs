# 星図の表示と観測地点

「操作」→「観測・表示設定」の「座標線・星座」と「恒星」で、全天図・地平図に描く項目を変更できます。赤道・銀河・地平座標は経緯線、黄道は黄緯0°の大円です。地平座標の方向は選んだ地点とシミュレーション日時に追随します。

選択したデータの `constellation_HIP.dat` を星座線・境界・名前、`constellations/*.cab` を星座絵に使います。原本の負の頂点番号で線画を区切り、へび座の2ファイルを同じ星座設定へ関連付けます。固有名は `star_names_HIP.dat` の日本語名、または英語名を使います。恒星色は各 HIP レコードの B−V、点の半径と不透明度は V 等級と明るさ係数から決まります。バイエル符号、フラムスティード番号、HIP 番号、二重星・系外惑星ホストのマーカーも選べます。

観測地点は `places_jp.dat`、`places.dat`、`defaultplaces_jp.dat`、`defaultplaces.dat` の順で最初に存在するファイルを使います。Places の文字列・数値リテラルを解析します。正規の `Planet.Earth.<地点>` を選択すると地平図の緯度・経度が変わります。手入力は地点との関連付けを解除します。

上記の星図設定と地名 ID は表示状態に保存できます。保存した地点の ID・座標、星名 HIP、個別星座の略号を読み込み時に照合します。Lua の `Grids.equatorial/ecliptic/galactic/horizontal`、`Constels.bounds/illusts`、`Stars.visible/labels/selectedLabels/bayerLabels/flamsteedLabels/hipLabels/brightness/markingType` を変更すると同じ画面に反映します。恒星の明るさには `brighten()`、`darken()`、`defaultBrightness()` も使えます。

実データの描画試験はビルド済みの C++ 移植元 f9fe624 の CAB 座標を3点比較し、両星図の描画・テセレーションとセッション往復を検証します。データの原本はコピーしません。

```sh
MITAKA_DATA_ROOT=/absolute/path/to/resources/data cargo test --locked -p mitaka-desktop real_catalog_layers -- --ignored --nocapture
```

## Lua からの選択・倍率操作

`Stars:select("32349 7588")` と `Stars:deselect("32349")` は固有名カタログの HIP を選択・解除します。どちらにも単独の `"All"` を指定できます。`Stars:isSelected("32349")` は単一の HIP 番号文字列を照合します。カタログに存在しない正の HIP は選択対象から外します。`Stars:setBrightnessTo(0.25, 2)` は2秒間の半余弦補間で明るさ係数を変更します。0秒で即時に適用し、`Stars.brightness` への代入で補間を止めます。

`Constels:hide("All")` の後で `Constels:show("And Ori")` を実行すると、アンドロメダ座とオリオン座の表示を選べます。`Constels:isVisible("Ori")` は星座ごとの選択状態を返します。線・名前・絵・境界の種類はそれぞれのスイッチで変更できます。略号は現カタログの綴りで指定します。不明な略号を含む更新はリスト全体をエラーにします。

`Planets:setScale(2, 3, 4, 5)` は移植元 f9fe624 と同じ順序で宇宙の惑星・月、宇宙の太陽、天球の惑星、天球の太陽・月の表示倍率を指定します。`Planets:getScale()` は同順の4値を返します。`Planets.scaleMode` と `getScaleMode()/setScaleMode()` は同じモードを使用します。標準の `mtk.KEEP_ORBIT` で4倍率を変更できます。`mtk.KEEP_RADIUS` では宇宙の2倍率の変更にカメラ距離補正が必要なため、移植元と同様にその更新を拒否し、天球の2倍率のみ変更できます。倍率の時間補間は未対応です。これらの選択・倍率・モードは表示状態に保存できます。

## ローカル HTML 操作パネル

「操作」→「ローカル HTML を開く」で単一の UTF-8 または CP932 文書を選びます。文書領域に見出し、段落、箇条書き、表の行・セルとローカル PNG/JPEG を描きます。外部 CSS は文書フォルダー以下の相対参照を8枚まで読み込みます。通常のタグ・クラス・IDと子孫・子セレクターに対して、文字色・背景色・サイズ・太字・斜体・下線を適用します。非表示要素、フレーム、スクリプトの内容は描画対象から外れます。背景画像・セル結合などのブラウザー固有のレイアウトは簡略表示です。

「このファイルの Lua リンクを自分の権限で実行する」を選び、本文中の下線付きの文字をクリックすると `mtkp://` を処理します。画像単独のリンクや別ウィンドウのリンクは Lua を起動しません。再確認時は選択時に読んだ HTML と CSS のファイル識別子と SHA-256 を照合します。変更・置き換えがあれば許可が失効します。外部スタイル、CSS のメディア条件や未対応の表示指定がある場合は読み取り専用になります。ネットワーク資源は取得しません。

旧 Plus 1.5.4 の `ui/index.html` を指定する比較試験では CP932 の5個の操作名と3枚の画像を照合します。通常の試験で、表内に描いた文字へのポインターイベント、非表示・フレーム・外部参照の抑止、ファイルの置き換え後の失効を検証します。

```sh
MITAKA_TEST_HTML=/absolute/path/to/mitakaPlus_154/ui/index.html cargo test --locked -p mitaka-desktop script_ui::document -- --include-ignored
```
