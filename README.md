# Goodhand

ピースの腕を、名盤ジャケットの強さ、彫版の点への執拗さ、UNIX の小道具の切れ味で端末へ届ける。形と構図を固定し、階調・影・光を独立して設計する。

## 現在の実装

中立な 2D 占有と別の gray8 を読む Rust reader、固定 pixel 面への配置、offline 抽出・符号化・再描画ツール。観測値と作者値は制作入力の役割を分けて扱う。端末アプリ、完成した作者階調、全周の美的評価、端末性能の検証はまだ完了していない。すべての[品質ゲート](DESIGN.md#品質保証のゲート)を通るまで 0.0.1 の完成や release 公開とは扱わない。

- runtime の追加依存なし。動画デコーダと Python は offline 制作用。
- Rust reader は入力を借用し、全 frame の構造検証後にのみ frame を公開する。parse/row iteration は heap 確保を行わない。
- `ShapeFrame` の占有と階調は別。黒い面と背景を同一視せず、形状に診断用の灰色を焼き込まない。
- 逆回転の順序はデータ側に保存。12 秒 / 24 fps / 288 frame は絶対時刻から選択し、dt を累積しない。
- 原動画・写真・作品データは同梱していない。合成入力のテストだけで検証できる。

## 構造

- [DESIGN.md](DESIGN.md): 完成像、契約、依存方向、品質ゲート、必要な仕事。
- [docs/tone.md](docs/tone.md): 一次資料に基づく階調・点・時間一貫性の設計と検証仮説。
- `src/frame.rs`: 占有、row/run の借用、呼出し側バッファへの 0/1 展開。
- `src/sequence.rs`: 形状入力検証と時刻からの frame 選択。
- `src/tone.rs`: shape とは別の occupied 順 gray8 の借用と検証。
- `src/surface.rs`: 論理点と gray8 を pixel 面へ配置。端末セルや I/O は扱わない。
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

# Python 3 / NumPy / Pillow が必要（offline 制作用のみ）:
python3 tools/test_pack_sequence.py
python3 tools/test_extract_reference.py
python3 tools/test_render_reference.py
python3 tools/test_pack_tone.py
```

ローカル検証の toolchain は Rust 1.100.0。CI は Python 合成テスト、Rust 1.100 の品質・library build、Rust 1.70 の互換目標を独立して検査する。各 commit の結果は Actions で確認する。Wasm の library build 成功とブラウザー上の動作・性能は別の検証とする。

## Offline 制作

`extract_reference.py` は明示選択した source hash、最初の video stream、288 個の PTS、24 fps を確認する。占有の候補、観測された輝度、位置付きの閾値曖昧点を別に保存する。観測輝度を元パレットや法線と呼ばない。

```sh
python3 tools/extract_reference.py source.mp4 observations.npz \
  --expected-sha256 SHA256 --direction reverse
python3 tools/pack_sequence.py observations.npz candidate.ghs --candidate
python3 tools/render_reference.py observations.npz redraw --frames 0 144
python3 tools/pack_tone.py observations.npz candidate.ghs observed-export \
  --kind observed --profile reference-v1
```

`--candidate` は未受入れの観測 mask を診断用に符号化する場合だけ使う。既定では受入れ済みでない mask を拒否する。抽出元、設定、位置付き曖昧点、形の比較を確認した上で受入れ記録を残す。無損失 round-trip は選んだ mask を保つ証明であって、形の正解や美的合格ではない。抽出・符号化・再描画ツールは既存の出力を上書きしない。参照再描画は同じ格子へ保存観測grayを配置するだけで、新しい照明や面を加えない。

再描画の出力先は未作成の専用ディレクトリを指定する。既存ディレクトリは空でも拒否し、途中失敗時の出力を別runへ混在させない。入力NPZは一度のsnapshotから検証・描画し、sidecarの時間・格子profileも照合する。

`pack_tone.py` は曲線適用や採用判定を行わず、選択した階調を `tone.ght` と別の由来記録 `tone.json` へ書き出す。`--kind observed` は `observed_gray`、`--kind authored` は制作側で合成・量子化済みの `authored_gray` を要求し、曖昧な `gray` や両役割の混在を拒否する。入力形式の詳細は[階調データの受渡し](docs/tone.md#制作値を実行形式へ渡す)を参照。

両役割とも独立した bool mask と shape を全セルで照合する。黒0も占有値として残す。入力NPZ・sidecar・shapeの同じsnapshotを検証・hash・符号化に使い、出力先は未作成の専用ディレクトリだけとする。maskの受入れ状態を階調の採用へ昇格させず、出力の `artifact_adoption` は常に `not_assessed`。形式への符号化成功と作品の採用は別である。

## Row-run 形式

候補形式 `GHSH` version 1。見た目を形式へ合わせるための制約ではない。

1. little-endian 16-byte header: magic `GHSH`, version u16, width u16, height u16, fps u16, frame_count u32。
2. payload 先頭からの `frame_count + 1` 個の u32 offset。
3. 各 frame は height 行。各行は u8 run count と、その個数の `(start: u8, length: u8)`。

width/height は 1..255、fps は 1..1000、frame_count は 1..4096。Rust reader は asset を 64 MiB 以下に制限する。offset は先頭 0、非減少、payload 内、最終値が payload 長と一致すること。各 run は非空・格子内・順序通りで、間に少なくとも一点の非占有を置く。隣接 run は packer 側で結合する。frame の余剰 byte を許さない。

検証は O(asset bytes)・確保なし、frame lookup は二つの offset、借用 row/run iteration は O(rows + runs)。N-byte mask 展開の API は N 個の出力位置を触り O(N + rows + runs) となる。この表現の費用であって、あらゆる表示の普遍的下限ではない。全周の bool 配列や毎 frame の全格子走査を、実行時の当然の仕事にはしない。


## Occupied gray8 と pixel 面

`GHT8` version 1 は `GHSH` と同じ 16-byte header レイアウト（magic は `GHT8`、version は 1）と offset table に、row/run 順の occupied gray8 を続ける。shape と寸法・fps・frame 数を照合し、各 frame の値数が run 長合計と一致することを全体検証する。asset は 64 MiB 以下。値 0 も有効な占有色であり、背景用の特別値ではない。shape hash と採用状態は offline provenance で対応付け、寸法・点数の一致だけで同一由来と断定しない。

```rust
let shape = goodhand::Sequence::from_bytes(&shape_bytes)?;
let tones = goodhand::ToneSequence::from_bytes(&tone_bytes, shape)?;
let spec = goodhand::RasterSpec::REFERENCE;
let mut pixels = vec![0u8; spec.required_len()?];
let f = shape.frame_index_at(elapsed); // 時計は一つだけ
// frame の範囲は同じ検証済み Sequence が決める。
goodhand::write_surface(shape.frame(f).unwrap(), tones.frame(f).unwrap(), spec, &mut pixels)?;
```

`REFERENCE` は 1000×640 **pixel**、原点(2,42)、pitch5、2×2点。`write_surface` は入力全体を確認してから背景を消去・配置し、失敗時には buffer を変更せず、成功時にも余剰 tail は保持する。呼出し側の確保を再利用し、関数内の heap 確保はない。P pixel の全面消去と occupied 点の footprint 書込みがこの実装の仕事であり、最小の実行方式や最速を保証しない。

これは端末 backend の前段で、文字セル、画像 protocol、配置、終了時復元はまだ実装していない。ANSI や Braille を元画像と同一と扱わず、使用端末での能力・構図・帯域を別に検証する。
