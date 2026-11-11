# 101 Çözücü

101 Okey oynarken masadaki durumu girdiğin, karşılığında o turda yapılacak en iyi hamleyi söyleyen bir yardımcı. Oyun değil: taşları sen masada oynarsın, program görülen bütün taşları sayar ve ne yapman gerektiğini adım adım yazar.

Tarayıcıda çalışır (telefon ve PC). Bütün hesap cihazın üzerinde yapılır; sunucu, hesap, internet gerekmez.

## Çalıştırma

`baslat.bat` dosyasına çift tıkla. `http://localhost:8101` açılır; siyah pencereyi kapatınca uygulama kapanır. Python kurulu olmalı (yalnızca `dist` klasörünü yayınlamak için kullanılıyor).

Telefonda kullanmak için `dist` klasörünü herhangi bir statik hosta koymak yeterli. Sayfa bir kez açıldıktan sonra ana ekrana eklenebilir ve çevrimdışı çalışır.

## Masada kullanım

Program her adımda tek bir soru sorar ve yalnızca o an mümkün olan seçenekleri gösterir.

1. **Gösterge** taşını seç. Okey buna göre belirlenir.
2. **Kimin başladığını** seç.
3. **Elindeki taşları** gir: önce renk, sonra sayı. Yanlış girdiğin taşa dokununca silinir.
4. Sıra sendeyken program **öneriyi** gösterir: hangi taşlar hangi pere, hangi uca (sol/sağ) işlenecek, hangi taş atılacak. Yaptıysan "Yaptım"a bas; başka bir şey yaptıysan "Diğer seçenekler"den seç ya da kendin gir.
5. Sıra rakipteyken ne yaptığını gir: desteden mi çekti, yandan mı aldı, ne açtı, neye işledi, ne attı.

Önerinin altında beklenen puan farkı, bitirme ihtimali ve attığın taşı sağdakinin alma ihtimali yazar. Program düşündükçe sayılar netleşir; beklemek zorunda değilsin.

Sayfanın altındaki kutular: atılan taşlar, elinin durumu ve işine yarayacak taşlar, görülmemiş taşlar. "Bu durumu kopyala" düğmesi o anki durumu metin olarak panoya alır; saçma bir öneri görürsen onu saklamak hatayı yeniden üretmeye yeter.

### Maç durumu

Varsayılan olarak program yalnızca oynanan ele bakar ve senin puanınla rakiplerinki arasındaki farkı açmaya çalışır. "Maç durumu" kutusunu açıp işaretlersen hedef değişir: **maçı kazanma ihtimali**.

Girilenler:

- kaçıncı elde olduğunuz ve toplam el sayısı,
- tekli oyunda sağdaki, karşıdaki ve soldaki oyuncudan kaç puan geride olduğun,
- eşli oyunda takım olarak kaç puan geride olduğunuz.

Öndeysen sayıyı eksi yaz. Yeni ele geçince el numarası kendiliğinden artar; puan farkını elle güncellersin.

Bu modda program gerideyken risk alır (gerekirse cezalı taş atar, işe yaramayan bir bitişi reddeder), öndeyken garantiye oynar. Öneride puan farkı yerine "Maçı kazanma ihtimali %X" görünür.

## Kurallar

Kuralların hepsi "Kurallar" ekranından değiştirilebilir. Varsayılanlar:

| Konu | Varsayılan |
|---|---|
| Dağıtım | 4 oyuncu, başlayan 22 diğerleri 21 taş |
| Açma | En az 101 puan per ya da en az 5 çift |
| Katlamalı | Kapalı. Açıkken her açılış bir öncekini geçmek zorunda (per ve çift eşikleri ayrı) |
| Perler | Seri 3–5 taş; 1 yalnızca 1-2-3'te kullanılır, 12-13-1 yok |
| Okey | Okey + tek taş çift sayılır, iki okey çift olmaz |
| İşleme | Tur başına bir perin her ucuna en fazla 2 taş, her çift açana en fazla 3 çift |
| Açmadan önce | Masaya hiçbir şey konmaz: işleme de okey alma da açtıktan sonra |
| Okey alma | Yerine gerçek taşı koyarak; grupta per 4 taşa tamamlanmalı |
| Yandan alma | Alınan taş o turda kullanılmak zorunda |
| Bitiş | Son taş atılarak bitilir; biten −101 |
| El sonu | Açmayan 101, açan elindeki taşların toplamı, çift açan iki katı, elde kalan okey +101 |
| Katlar | Okey atarak bitme, elden bitme, çiftle bitme: her biri ×2 (birbiriyle çarpılır) |
| Cezalar | İşlek taş atmak 101, okey atmak 101, perindeki okeyi rakibe kaptırmak 101 (perin sahibine) |
| Eşli | Ortağın bitince takıma −202 |

## Öneri nasıl bulunuyor

1. **Bütün yasal hamleler üretilir.** Açma, çift açma, işleme, okey alma ve atılacak taşın her birleşimi tek tek çıkarılır. Çoğu turda bu 20 civarı hamledir, kalabalık ellerde binleri bulur.
2. **Bilinmeyen taşlar rastgele dağıtılır.** Rakiplerin eli ve deste, görülmemiş taşlardan çekilir. Rakibin attığı ve almadığı taşlar elinde ne olabileceğine dair ipucu olarak kullanılır.
3. **Her hamle aynı dağıtımlarda sonuna kadar oynatılır.** Hamleden sonrasını botlar oynar; bütün hamleler aynı dağıtımları gördüğü için aradaki fark şansa değil hamleye aittir.
4. **Açıkça geride kalan hamleler elenir**, kalan süre yakın olanlara harcanır.
5. **Sıralama** el modunda ortalama puan farkına, maç modunda maçı kaybetme ihtimaline göre yapılır.

Hesap tarayıcıda çekirdek sayısı − 1 işçiyle (en fazla 12) paralel yürür.

## Ölçümler

**Güç.** Lig: 700 el, karar başına 80 simülasyon, düz bot 1000 Elo kabul edilerek.

| Oyuncu | 11 ellik oyun | Tek el |
|---|---|---|
| Herkesin elini gören oyuncu (tavan) | 1293 | 1078 |
| Bu program | 1223 | 1072 |
| Ayarlanmış bot | 1051 | 1020 |
| Düz bot | 1000 | 1000 |

Tek elde şans çok baskın olduğu için herkesin elini gören oyuncu bile 1080'i geçemiyor. Belirsizlik oyuncu başına kabaca ±40 Elo.

**Maç durumu.** Son elde, her fark için 300 el, ayarlanmış botlara karşı maçı kazanma oranı:

| Herkesten geride | Maça göre oynayan | Ele göre oynayan |
|---|---|---|
| −200 (önde) | %88,2 | %88,5 |
| 0 | %38,4 | %33,4 |
| 100 | %21,3 | %16,3 |
| 200 | %3,3 | %3,7 |
| 300 | %0,7 | %0,7 |
| 400 | %0,3 | %0,3 |

Maç yakınken yaklaşık 5 puan kazandırıyor. Çok öndeyken ya da çok gerideyken ölçülebilir bir fark yok (satır başına gürültü yaklaşık ±2,5 puan). Bu ölçüm yalnızca tekli oyunda yapıldı.

**Hız** (tek çekirdek): düz bot yaklaşık 5400 el/sn, ayarlanmış bot 3800 el/sn; 12 açılış pozisyonu × 150 dağıtım 4,6 sn.

## Bilinen sınırlar

- Hamleden sonrasını botlar oynadığı için program, botların göremediği ince planları da göremez. Kendi sonraki turlarını aramak yalnızca maçın son elinde devrede.
- Rakip modeli basit: blöfü ve masadaki oyuncunun tarzını bilmez.
- Simülasyondaki ortak sıradan bir bot gibi oynar; sana göre taş atmaz.
- 20 000'den fazla yasal hamlesi olan ellerde tam liste yerine hazır planlar karşılaştırılır.

## Proje yapısı

```
crates/engine   saf Rust motor
  tile.rs, rules.rs, table.rs   taşlar, kurallar, masadaki perler
  meld.rs, pairs.rs             bir elden en iyi per / çift dizilimi
  game.rs, policy.rs            oyun durumu, puanlama, botlar ve ayarları
  moves.rs                      bir turdaki bütün yasal hamleler
  analysis.rs                   hamleleri karşılaştıran analiz
  stakes.rs                     maç durumu ve kazanma ihtimali
  agent.rs                      ortak oyuncu arayüzü, Elo hesabı
crates/app      Leptos arayüzü ve analizi yapan web worker'lar (WASM)
dist            derlenmiş uygulama
```

## Geliştirme

Gerekenler: Rust, `wasm32-unknown-unknown` hedefi, [trunk](https://trunkrs.dev).

```sh
cd crates/app
trunk serve            # http://127.0.0.1:8080, değişiklikte yeniden derler
trunk build --release  # ../../dist
```

```sh
cargo test --release -p okey-engine
cargo test -p okey-app
cargo clippy --workspace --all-targets
```

`tests/rules.rs` her masa kuralı için bir test içerir; kural eklerken ya da değiştirirken ilk bakılacak yer orası. `tests/engine.rs` içindeki `bot_play_is_unchanged` bot davranışını kilitler: motoru yalnızca hızlandıran bir değişiklikten sonra geçmelidir; davranış bilerek değiştiyse içindeki sayılar güncellenir.

Ölçüm araçları (`cargo run --release -p okey-engine --example <ad> -- <argümanlar>`):

| Araç | Ne ölçer |
|---|---|
| `bench` | Hız: bot elleri/saniye ve bir pozisyonun analiz süresi |
| `arena [el] [simülasyon]` | Aramanın düz bota göre el başına puan kazancı |
| `league [el] [simülasyon] [oyun başına el]` | Bütün oyuncular arasında Elo (tek el ve tam oyun) |
| `comeback [el] [simülasyon]` | Son elde geriden gelip maçı kazanma oranı, maç modu açık ve kapalı |
| `tune [tur] [deneme] [el]` | Bot ayarlarını kendi kendine oynatarak iyileştirir |
| `gen_stats` | Hamle üreticinin bulduğu hamle sayıları ve süresi |
