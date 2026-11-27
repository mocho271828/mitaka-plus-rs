# 実装方針

本番投入前の新規プロジェクト。比較対象は Mitaka_plus の f9fe62485ad45a3b5651e050a49871a71b6f351d とする。独自実装には Rust を使い、OSのAPIと固定版Luaを外部コンポーネントとして扱う。

- [移植元とデータの保全](policy_gates/data.md)
- [共有実装とOS境界](policy_gates/platform.md)
- [ビルドと検証](policy_gates/testing.md)

配布ごとに3 OSの展開済み実行ファイルを検査し、本体の主画面と投影画面は配布物から描画する。Ubuntuの描画はWaylandとX11の両方を検査する。比較範囲と実機・CIの検証条件は [移植範囲と検証](docs/COMPATIBILITY.md) に記録する。

レビュー時には3ファイルを temp_gates/YYYY-MM-DD-HH-MM-SS にコピーし、実際に検証した条件と結果を記入する。
