# mierucover

エクスプローラーで、漫画・同人誌の書庫と電子書籍に表紙のサムネイルを出す（「見えるカバー」）。

対象: `.zip` `.cbz` `.rar` `.cbr` `.7z` `.cb7` `.tar` `.cbt` `.epub` `.azw3` `.azw` `.mobi`

- 表紙の 1 枚だけを読むので速い。zip は書庫が大きくても数十ミリ秒で出る
- 表紙らしい画像を選ぶ。名前が表紙を示す画像を優先し、ストアの紹介画像を飛ばし、見開きの表紙は表紙側だけを切り出す
- 設定画面も常駐もなく、DLL 1 つだけ（ランタイムのインストールも不要）

Windows 10 / 11（64 ビット）用。動作の確認は Windows 11 でだけしている。

## 入れ方

1. [Releases](https://github.com/mlkami/mierucover/releases) から `mierucover-<版>.zip` を落として展開する
2. 展開したフォルダで PowerShell を開き、次を実行する（管理者権限は要らない）

   ```powershell
   powershell -ExecutionPolicy Bypass -File install.ps1
   ```

3. エクスプローラーで書庫のあるフォルダを開き、表示を「中アイコン」以上にする

- ArcThumb など、ほかの書庫サムネイルのソフトを使っているなら、先にアンインストールする（同じ登録を取り合うため）
- 前のソフトが作ったサムネイルが残って出るときは、`tools\clear_thumbnail_cache.ps1` で Windows のサムネイルキャッシュを消す。写真などのサムネイルも作り直しになり、開いているエクスプローラーは一度閉じる
- 署名のない DLL なので、Smart App Control が有効な PC では読み込みを止められるかもしれない（未確認）。ウイルス対策ソフトが誤って検知することもありうる
- DLL は `%LOCALAPPDATA%\Programs\mierucover` に入り、レジストリは `HKCU\Software\Classes` の下だけを使う。登録するのは `CLSID\{9F2321AF-6F5F-43B4-8A26-F7A29D74B5BA}` と、各拡張子の `ShellEx\{e357fccd-a995-4576-b01f-234630154e96}`

消すときは、展開したフォルダで次を実行する。mierucover が書いた登録とファイルだけを消す。

```powershell
powershell -ExecutionPolicy Bypass -File uninstall.ps1
```

### サムネイルが消えないようにする（任意）

Windows の自動のディスク クリーンアップは、サムネイルのキャッシュを消すことがある。消えると、次にフォルダを開いたときに全部作り直しになる。管理者の PowerShell で次を実行すると、自動では消さなくなる（手動のディスク クリーンアップで「縮小表示」を選べば、今まで通り消せる）。

```powershell
$k = 'Microsoft\Windows\CurrentVersion\Explorer\VolumeCaches\Thumbnail Cache'
Get-ItemProperty "HKLM:\SOFTWARE\$k" -Name Autorun   # 戻すときのために元の値を見ておく
Set-ItemProperty "HKLM:\SOFTWARE\$k" -Name Autorun -Value 0
Set-ItemProperty "HKLM:\SOFTWARE\WOW6432Node\$k" -Name Autorun -Value 0
```

## 表紙の選び方

1. 形式は拡張子ではなく中身で判定する（中身が zip の `.cbr` なども読める）
2. 画像をエクスプローラーと同じ自然順（`2.jpg` < `10.jpg`、`_001` < `001` < `001a`）に並べ、先頭を表紙にする
3. 名前が表紙を示す画像（`cover`、`hyoushi`、`表紙` など）は先頭に回す。`裏表紙`、`back`、`page` を含むものは除く
4. 先頭が小さい横長の画像（DLsite や FANZA の紹介画像、560×420 など）で、次が縦長なら、次を表紙にする
5. 横長の表紙は、表紙の部分だけを切り出す
   - 縦横比 1.8 以上（折り返し付きのカバー全体）、または 1.2 以上で次のページが縦長（表紙＋裏表紙の見開き）のとき
   - 表紙が左右どちらにあるかは本による。背表紙の両側の表紙大の範囲を比べて、色が豊かで細かい方を表紙とする。差がはっきりしないときは切らずに全体を出す
6. EPUB は本が指定した表紙を使う（SVG やページを指していれば、その中の画像）。EPUB や書庫を束ねた書庫は、先頭の 1 冊の表紙を使う
   - ソリッド圧縮の 7z・RAR で、先頭の画像の手前に 32MB 超の展開が要るときは、手前の範囲で自然順が一番前の画像を使う（7-Zip は拡張子順に格納するため、先頭の画像が末尾にあることがある。複数巻の書庫では別の巻の表紙になる）
7. AZW3・AZW・MOBI は本が指定した表紙（EXTH 201）を使う

## 速さのための作り

- zip は末尾の目次と表紙 1 枚だけを読む。書庫の大きさに関係なく速い
- JPEG は 1/2・1/4・1/8 のサイズのまま直接デコードする（WIC）。プログレッシブ JPEG は最初の段階だけ読む
- 画像の読み込みは Windows の WIC。WebP・AVIF・HEIC は Windows の拡張機能があれば読める。新しい形式も拡張機能を入れれば読める
- エクスプローラーとは別のプロセスで動く（IInitializeWithStream）。壊れた書庫でエクスプローラーが落ちることはない

## 開発

Rust（依存ライブラリの都合で 1.97.1 以上）が要る。ソースのフォルダで `install.ps1` を実行すると、ビルドしてから入れる（更新も同じ）。表紙の選び方を変えた後は、Windows のサムネイルキャッシュに古いものが残るので、`tools\clear_thumbnail_cache.ps1` で作り直す。

```powershell
cargo test --release                                        # 単体テストと、ダミー書庫での結合テスト
powershell -ExecutionPolicy Bypass -File tools\make_fixtures.ps1   # ダミー書庫を target\fixtures に作る（7-Zip が必要）
target\release\mcdev.exe scan <フォルダ> [--render] [--out 結果.tsv]   # 表紙の選び方と時間を一覧にする（画像は保存しない）
target\release\mcdev.exe thumb <ファイル> <出力.png>          # サムネイルを PNG に書き出す
target\release\mcdev.exe com <フォルダ>                      # エクスプローラーと同じ COM 経由で時間を測る
powershell -ExecutionPolicy Bypass -File tools\package.ps1         # 配布用の zip を target に作る
powershell -ExecutionPolicy Bypass -File tools\third_party_notices.ps1   # 依存を変えたら THIRD-PARTY-NOTICES.txt を作り直す
```

## ライセンス

MIT（`LICENSE`）。DLL に入っているライブラリのライセンスは `THIRD-PARTY-NOTICES.txt` にまとめてある。RAR は [unrar-rs](https://github.com/scryer-media/rarpar)（unRAR のソースを元にした Rust 実装）で読む。こちらは Apache-2.0 に unRAR のライセンスの条件が付く（`licenses\unrar-rs-LICENSE.txt`）。
