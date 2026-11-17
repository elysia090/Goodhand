# Goodhand

ピースの腕を、名盤ジャケットの強さ、彫版の点への執拗さ、UNIX の小道具の切れ味で端末へ届ける。形と構図を固定し、階調・影・光を独立して設計する。

## 現在の実装

中立な 2D 占有データの Rust reader と offline 抽出・符号化ツール。端末アプリ、完成した作者階調、全周の美的評価、端末性能の検証はまだ完了していない。すべての[品質ゲート](DESIGN.md#品質保証のゲート)を通るまで 0.0.1 の完成や release 公開とは扱わない。

- runtime の追加依存なし。動画デコーダと Python は offline 制作用。
- Rust reader は入力を借用し、全 frame の構造検証後にのみ frame を公開する。parse/row iteration は heap 確保を行わない。
- `ShapeFrame` の占有と階調は別。黒い面と背景を同一視せず、形状に診断用の灰色を焼き込まない。
- 逆回転の順序はデータ側に保存。12 秒 / 24 fps / 288 frame は絶対時刻から選択し、dt を累積しない。
- 原動画・写真・作品データは同梱していない。合成入力のテストだけで検証できる。

## 構造

- [DESIGN.md](DESIGN.md): 完成像、契約、依存方向、品質ゲート、必要な仕事。
- [docs/tone.md](docs/tone.md): 一次資料に基づく階調・点・時間一貫性の設計と検証仮説。
- `src/frame.rs`: 占有、row/run の借用、呼出し側バッファへの 0/1 展開。
- `src/sequence.rs`: 入力検証と時刻からの frame 選択。
- `tools/`: 参照映像の観測と制作時の符号化。実行時には不要。

`Sequence::from_bytes(&bytes)` は全 offset/row/run を検証する。`frame(index)` は範囲を検査し、`frame_at(Duration)` は時計から直接選択する。`ShapeFrame::rows()` は全格子へ展開せずに区間を参照する。`write_mask(&mut output)` は先頭 width × height byte だけに 0/1 を書き、不足したバッファは変更しない。

## 検証

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
cargo test --release
cargo build --release --lib
# target が導入済みの場合:
cargo build --release --lib --target wasm32-unknown-unknown

# Python 3 / NumPy が必要:
python3 tools/test_pack_sequence.py
python3 tools/test_extract_reference.py
```

検証した toolchain は Rust 1.100.0。Cargo の Rust 1.70 指定は互換性の目標で、1.70 での実行試験は未実施。Wasm の library build 成功とブラウザー上の動作・性能は別の検証とする。

## Offline 制作

`extract_reference.py` は明示選択した source hash、最初の video stream、288 個の PTS、24 fps を確認する。占有の候補、観測された輝度、位置付きの閾値曖昧点を別に保存する。観測輝度を元パレットや法線と呼ばない。

```sh
python3 tools/extract_reference.py source.mp4 observations.npz \
  --expected-sha256 SHA256 --direction reverse
python3 tools/pack_sequence.py observations.npz candidate.ghs --candidate
```

`--candidate` は未受入れの観測 mask を診断用に符号化する場合だけ使う。既定では受入れ済みでない mask を拒否する。抽出元、設定、位置付き曖昧点、形の比較を確認した上で受入れ記録を残す。無損失 round-trip は選んだ mask を保つ証明であって、形の正解や美的合格ではない。両ツールは既存の出力を上書きしない。

## Row-run 形式

候補形式 `GHSH` version 1。見た目を形式へ合わせるための制約ではない。

1. little-endian 16-byte header: magic `GHSH`, version u16, width u16, height u16, fps u16, frame_count u32。
2. payload 先頭からの `frame_count + 1` 個の u32 offset。
3. 各 frame は height 行。各行は u8 run count と、その個数の `(start: u8, length: u8)`。

width/height は 1..255、fps は 1..1000、frame_count は 1..4096。Rust reader は asset を 64 MiB 以下に制限する。offset は先頭 0、非減少、payload 内、最終値が payload 長と一致すること。各 run は非空・格子内・順序通りで、間に少なくとも一点の非占有を置く。隣接 run は packer 側で結合する。frame の余剰 byte を許さない。

検証は O(asset bytes)・確保なし、frame lookup は二つの offset、借用 row/run iteration は O(rows + runs)。N-byte mask 展開の API は N 個の出力位置を触り O(N + rows + runs) となる。この表現の費用であって、あらゆる表示の普遍的下限ではない。全周の bool 配列や毎 frame の全格子走査を、実行時の当然の仕事にはしない。
