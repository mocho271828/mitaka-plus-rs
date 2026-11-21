# 星図の表示と観測地点

「操作」→「観測・表示設定」の「座標線・星座」と「恒星」で、全天図・地平図に描く項目を変更できます。赤道・銀河・地平座標は経緯線、黄道は黄緯0°の大円です。地平座標の方向は選んだ地点とシミュレーション日時に追随します。

選択したデータの `constellation_HIP.dat` を星座線・境界・名前、`constellations/*.cab` を星座絵に使います。原本の負の頂点番号で線画を区切り、へび座の2ファイルを同じ星座設定へ関連付けます。固有名は `star_names_HIP.dat` の日本語名、または英語名を使います。恒星色は各 HIP レコードの B−V、点の半径と不透明度は V 等級と明るさ係数から決まります。バイエル符号、フラムスティード番号、HIP 番号、二重星・系外惑星ホストのマーカーも選べます。

観測地点は `places_jp.dat`、`places.dat`、`defaultplaces_jp.dat`、`defaultplaces.dat` の順で最初に存在するファイルを使います。Places の文字列・数値リテラルを解析します。正規の `Planet.Earth.<地点>` を選択すると地平図の緯度・経度が変わります。手入力は地点との関連付けを解除します。

上記の星図設定と地名 ID は表示状態に保存できます。保存した地点の ID・座標、星名 HIP、個別星座の略号を読み込み時に照合します。Lua の `Grids.equatorial/ecliptic/galactic/horizontal`、`Constels.bounds/illusts`、`Stars.visible/labels/selectedLabels/bayerLabels/flamsteedLabels/hipLabels/brightness/markingType` を変更すると同じ画面に反映します。恒星の明るさには `brighten()`、`darken()`、`defaultBrightness()` も使えます。

実データの描画試験はビルド済みの C++ 移植元 f9fe624 の CAB 座標を3点比較し、両星図の描画・テセレーションとセッション往復を検証します。データの原本はコピーしません。

```sh
MITAKA_DATA_ROOT=/absolute/path/to/resources/data cargo test --locked -p mitaka-desktop real_catalog_layers -- --ignored --nocapture
```
