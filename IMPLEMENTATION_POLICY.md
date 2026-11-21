# 実装方針

本番投入前の新規プロジェクト。比較対象は Mitaka_plus の f9fe62485ad45a3b5651e050a49871a71b6f351d とする。独自実装には Rust を使い、OSのAPIと固定版Luaを外部コンポーネントとして扱う。

- [移植元とデータの保全](policy_gates/data.md)
- [共有実装とOS境界](policy_gates/platform.md)
- [ビルドと検証](policy_gates/testing.md)

レビュー時には3ファイルを temp_gates/YYYY-MM-DD-HH-MM-SS にコピーし、実際に検証した条件と結果を記入する。
