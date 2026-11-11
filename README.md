# 101 Çözücü

101 Okey için hamle analizi. Tarayıcıda çalışır (telefon ve PC), her şey cihazda hesaplanır.

## Kullanım

`baslat.bat` dosyasına çift tıkla; `http://localhost:8101` açılır. Pencereyi kapatınca uygulama kapanır.

## Yapı

- `crates/engine` — saf Rust motor
  - `rules.rs` masa kuralları (hepsi ayarlanabilir), `tile.rs`, `table.rs`
  - `meld.rs` bir elden en iyi per dizilimini bulan arama
  - `game.rs` oyun durumu, botlar, puanlama
  - `policy.rs` bot davranışının ayarları (`BASIC`, kendi kendine oynayarak ayarlanmış `TUNED`)
  - `moves.rs` bir turda yapılabilecek bütün yasal hamleleri üretir
  - `analysis.rs` hamleleri rastgele dağıtımlarda karşılaştıran analiz
  - `agent.rs` ortak oyuncu arayüzü (bot, arama, ileride eğitilmiş model) ve Elo hesabı
- `crates/app` — Leptos arayüzü ve analizi yapan web worker'lar (WASM)

## Geliştirme

```sh
cd crates/app
trunk serve            # http://127.0.0.1:8080
trunk build --release  # ../../dist, herhangi bir statik hostta çalışır
```

```sh
cargo test --release -p okey-engine     # tests/rules.rs: her masa kuralı için bir test
cargo test -p okey-app --bin app
cargo clippy --release --workspace --all-targets
```

Ölçüm araçları (`cargo run --release -p okey-engine --example <ad>`):

| Araç | Ne ölçer |
|---|---|
| `bench` | Hız: bot elleri/saniye ve bir pozisyonun analiz süresi |
| `arena [el] [simülasyon]` | Arama sürümlerinin düz bota göre el başına puan kazancı |
| `league [el] [simülasyon] [oyun başına el]` | Bütün oyuncular arasında Elo (tek el ve tam oyun) |
| `tune [tur] [deneme] [el]` | Bot ayarlarını kendi kendine oynatarak iyileştirir |
| `gen_stats` | Hamle üreticinin bulduğu hamle sayıları ve süresi |

`tests/engine.rs` içindeki `bot_play_is_unchanged` testi bot davranışını kilitler: motoru yalnızca
hızlandıran bir değişiklikten sonra geçmeli; davranış bilerek değiştiyse içindeki sayılar güncellenir.

## Son ölçümler

- Lig (700 el, karar başına 80 simülasyon, düz bot = 1000): tam oyun (11 el) Elo'su arama 1223,
  ayarlanmış bot 1051, herkesin elini gören oyuncu 1293. Tek el Elo'sunda tavan yaklaşık 1080.
- Hız (tek çekirdek): düz bot 5500 el/sn, ayarlanmış bot 3800 el/sn; 12 açılış pozisyonu × 150 dağıtım 4.5 sn.
  Tarayıcıda analiz çekirdek sayısı − 1 işçiyle (en fazla 12) paralel çalışır.
