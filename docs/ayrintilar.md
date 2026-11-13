# Ayrıntılar

## Masada kullanım

Ekran ikiye ayrılır: masa (rakipler, son attıkları taşlar, açılan perler, istakan) ve yanında o an sorulan tek soruyu soran panel. Telefon yan tutulunca panel sağda, dik tutulunca altta durur.

1. **Gösterge**, **kim başlıyor** ve **taşların** sorulur. Taşlar 4 × 13'lük taş seçiciden tek dokunuşla girilir; bir taşın son kopyası kaldığında altında nokta çıkar, kalmadığında soluklaşır. Yanlış girdiğin taşa yeşil alanda dokunursan silinir.
2. Sıra sendeyken panel **öneriyi** gösterir; aynı hamle istakanın üstüne de çizilir: aynı renkle çevrili taşlar birlikte indirilir, numaralar sırayı verir, atılacak taş yukarı kalkar. Yaptıysan "Yaptım"a bas; başka bir şey yaptıysan alttaki seçeneklerden birine dokun ya da istakadan kendin seç.
3. Çekme sırası sendeyken panel desteden çekmeyle yandan almayı karşılaştırır ve **destenin son taşının kime gideceğini** gösterir: herkes desteden çekerse kesin cevap, ve simülasyonlarda gerçekte kimin çektiği.
4. Sıra rakipteyken istakanın yerine taş seçici gelir: önce desteden mi çekti yandan mı aldı, sonra attığı taş. Per indirdiyse taşlarını sırayla girersin; işlediyse masada o pere dokunursun.

Önerinin altında beklenen puan farkı, bitirme ihtimali ve attığın taşı sağdakinin alma ihtimali yazar. Program düşündükçe sayılar netleşir; beklemek zorunda değilsin.

Üst çubuktaki ☷ düğmesi atılan taşları, elinin durumunu ve görülmemiş taşları açar; ⚙ ayarları ve maç puanını. "Bu durumu kopyala" düğmesi o anki durumu metin olarak panoya alır; saçma bir öneri görürsen onu saklamak hatayı yeniden üretmeye yeter.

### Maç puanı

El bitince program puan tablosunu açar. Senin puanın elinden hesaplanır; açmayan rakiplere 101 yazılır, açanlar için elinde kalan puanı girersin (çift açanınki iki katı alınır). El boyunca yazılan cezalar (işlek, okey atma, okeyi kaptırma, 7 çiftle açma) ve bitirenin katları (okey atarak, elden, çiftle) kendiliğinden eklenir. "Yeni el" puanları maç toplamına ekler; üst çubukta el numarası ve liderle aradaki fark görünür. Toplamlar ayarlardan düzeltilebilir, yeni maç da oradan başlar.

Program maç boyunca farkı açmaya oynar. (Maç hedefini bütün ellere yaymayı ölçtüm: 5. elden sonra 400 geride, 300 önde ve sıfırdan başlayan maçlarda daha çok maç kazandırmadı, puan kaybettirdi; bu yüzden yalnızca son iki elde devrede.) Maçın son iki elinde hedefi **maçı kazanma ihtimali** olur: gerideyken risk alır (gerekirse cezalı taş atar, işe yaramayan bir bitişi reddeder), öndeyken garantiye oynar. O ellerde öneride puan farkı yerine "Maçı kazanma ihtimali %X" görünür ve yapay zekâ devre dışı kalır.

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
| Cezalar | İşlek taş atmak 101, okey atmak 101, masaya koyduğun okeyi rakibe kaptırmak 101 (okeyi koyana; kendi koyduğunu geri almak cezasız) |
| Eşli | Ortağın bitince takıma −202 |

## Öneri nasıl bulunuyor

1. **Bütün yasal hamleler üretilir.** Açma, çift açma, işleme, okey alma ve atılacak taşın her birleşimi tek tek çıkarılır. Çoğu turda bu 20 civarı hamledir, kalabalık ellerde binleri bulur.
2. **Bilinmeyen taşlar rastgele dağıtılır.** Rakiplerin eli ve deste, görülmemiş taşlardan çekilir. Rakibin attığı ve almadığı taşlar elinde ne olabileceğine dair ipucu olarak kullanılır.
3. **Her hamle aynı dağıtımlarda oynatılır.** Rakipleri botlar, senin bir sonraki turunu eğitilmiş model oynar; simülasyon orada kesilir ve elin geri kalanı için modelin biçtiği değer alınır. Bütün hamleler aynı dağıtımları gördüğü için aradaki fark şansa değil hamleye aittir. Dört dağıtımdan biri yine elin sonuna kadar oynanır; bitirme ihtimali ve son taşın kime gittiği yalnızca onlardan hesaplanır.
4. **Açıkça geride kalan hamleler elenir**, kalan süre yakın olanlara harcanır.
5. **Sıralama** el modunda ortalama puan farkına, maç modunda maçı kaybetme ihtimaline göre yapılır.

Hesap tarayıcıda çekirdek sayısı − 1 işçiyle (en fazla 12) paralel yürür. İstatistiği tek bir yönetici tutar: işçiler yalnızca istenen hamleleri dağıtımlarda oynatıp sonucu gönderir, hangi hamlenin eleneceğine yönetici bütün işçilerin sonuçlarına bakarak karar verir. Öndeki hamleyi hiçbir rakibi bir puandan fazla geçemeyecek hâle gelince (en az 600 dağıtımdan sonra, üç standart hatayla) hesap durur; 80 pozisyonda bu kuralın 12 000 dağıtımlık hesaba göre ölçülebilir bir kaybı çıkmadı. Derleme WebAssembly SIMD kullanır (Chrome 91, Firefox 89, Safari 16.4 ve sonrası).

## Yapay zekâ modeli

Gördüğün durumu alıp "bu hamleden sonra el sonunda rakiplerle aramdaki puan farkı ne olur" sorusuna tek sayı veren bir değer ağı: 804 açık/kapalı girdi ve 23 sayı, ardından 256, 128 ve 64 hücrelik üç katman. Tarayıcıda çalışır. Rakiplerin elini ve destenin sırasını görmez; `tests/model.rs` bunu sınar.

Simülasyonlarda senin sonraki turunu o oynar ve vardığı durumu o değerlendirir (Kurallar ekranındaki "Yapay zekâ" kutusu, varsayılan açık). Tekli ve eşli oyun için ayrı birer ağ vardır (`crates/app/model/value.bin`, `partners.bin`). Varsayılan kurallarda devreye girer, maçın son iki eli hariç; diğer durumlarda arama eskisi gibi botla devam eder.

**Nasıl eğitildi.** Farklı tarzda botlar el oynar; her kararda birkaç seçenek gerçek dağıtım üzerinde elin sonuna kadar oynatılır (deste 8 kez yeniden karıştırılıp ortalaması alınır) ve "oyuncunun gördüğü → sonuç" kaydedilir. İlk turda seçeneklerden sonrasını bot oynar; sonraki turlarda bir önceki model oynar, böylece model "ileride iyi oynayacağımı" varsayarak değer biçmeyi öğrenir. Tekli ve eşli için üçer tur yapıldı:

| Tur | Sonrasını oynayan | Tekli, model tek başına | Eşli, model tek başına |
|---|---|---|---|
| 1 | ayarlı bot | +13,7 | +24,9 |
| 2 | 1. tur modeli | +20,4 | +38,3 |
| 3 | 2. tur modeli | +21,8 | +38,7 |

(Ayarlı bota göre el başına puan kazancı, eşlide takım farkı; 40 000 el, tekli ± 0,5, eşli ± 1,4.)

Tekli ağ için dördüncü bir tur daha yapıldı. Bu turda seçenekler elin sonuna kadar değil, oyuncunun bir sonraki turuna kadar oynatıldı ve orada 3. tur modelinin değeri alındı (24 deste sırası); model koltuklarının rakip olduğu oyunlarda onları da model oynadı. Etiketler çok temizlendi: bir kararın seçenekleri arasındaki farkın açıklanabilen kısmı %28'den %57'ye çıktı. Oyun gücü ise az arttı: model tek başına bitmeye oynayan botlara karşı +23,4'ten +24,5'e (± 0,6), model masasına karşı +1,5 (± 0,7). Uygulamadaki tekli ağ bu 4. tur ağıdır.

Girdilerde "herkes desteden çekerse son taş kime gelir" bilgisi de var; yandan taş alıp son taşı kaydırma taktiği için eklendi. Uygulamadaki sade tekli ağ bununla yeniden eğitilince tek başına +20,0'dan +21,2'ye (± 0,6) çıktı.

**Denenip işe yaramayanlar.** Daha büyük ya da daha derin ağ (temiz etiketlerle de), daha uzun eğitim, taşları renk × sayı ızgarası olarak işleyen mimari, veriyi iki katına çıkarmak, modelin görüşünü aramaya ön bilgi olarak vermek: hiçbiri ölçülebilir fark yaratmadı. Daha küçük ağlar 1–3,5 puan kaybettirdi. Simülasyonda rakipleri de modelin oynaması, model masasına karşı bile fark yaratmadı (+0,4 ± 3,7). Simülasyonu bir yerine iki tur sonra kesmek ve çizim girdili büyük ağı kullanmak 640 dağıtımda aynı sonucu verdi (−2,7 ± 3,2 ve +0,3 ± 3,1).

**Rakibin elini tahmin etmek.** Rakip ellerini bilen arama +26 puan daha iyi oynuyor (sağdakinin eli tek başına +8, karşıdaki +1, soldaki +3; üçünü bilmek destede ne kaldığını da söylediği için toplamından fazla). Ama açık bilgiden bunun çıkarılabilen kısmı çok küçük: rakibin attığı taş, almadığı taş, açmamış olması ve elinde işlenecek taş bırakmaması tek tek anlamlı ipuçları (gerçek elde 2–3 kat daha sık), hepsi birlikte gizli taşların yerindeki belirsizliğin yalnızca %2'sini gideriyor. Daha önce denenen el okuma ağı %4,7'de kalmıştı. Aramadaki basit tahmin kuralının katkısı da ölçülemeyecek kadar küçük (+2,1 ± 3,1).

## Ölçümler

**Bugünkü durum** (`versus`: oyuncu üç rakibe karşı oturur, rakipler elden ele yer değiştirir; değer, rakiplerin ortalamasına göre el başına puan farkı; 770 el, maç 11 el, şansla kazanma %25):

| Masa | Oyuncu | El başına fark | Maçı kazanma |
|---|---|---|---|
| 3 bitmeye oynayan bot | aynı bot | −1 ± 1 | %24 |
| | model tek başına, anlık | +24,5 ± 0,7 | %55 |
| | arama, sonuna kadar oynatarak, 80 dağıtım (eski) | +38,1 ± 3,9 | %69 |
| | arama, bir tur sonra keserek, 80 dağıtım | +45,6 ± 4,1 | %75 |
| | **arama, bir tur sonra keserek, 640 dağıtım** | **+51,4 ± 4,0** | **%81** |
| | rakip ellerini gören arama (hile) | +71,5 ± 4,4 | %91 |
| | elleri ve deste sırasını gören (hile) | +123 ± 5 | %100 |
| 3 model (güçlü, dürüst) | bitmeye oynayan bot | −22,5 ± 1,1 | %6 |
| | arama, sonuna kadar oynatarak (eski) | +6,9 ± 3,9 | %29 |
| | arama, bir tur sonra keserek | +6,6 ile +10,5 arası ± 4 | %31–35 |
| 1 her şeyi gören + 2 bot | bitmeye oynayan bot | görene karşı −132 | %1 |
| | model tek başına | görene karşı −124 | %2 |
| | arama, 320 dağıtım | görene karşı −33 ± 5, botlara +43 | %41 |
| Eşli, eş ve rakipler ayarlı bot | arama, sonuna kadar oynatarak (eski) | takım farkı +46,9 ± 9,6 | %72 |
| | **arama, bir tur sonra keserek** | **takım farkı +76,9 ± 9,6** | **%84** |

Kesmenin aynı ellerdeki kazancı tekli oyunda +7,5 ± 3,6, eşlide +30,0 ± 9,7; hesap da yaklaşık iki kat hızlandı. 320 dağıtımdan sonra kazanç doyuyor. "Her şeyi gören" rakip bütün elleri ve destenin sırasını bilir, botların ne yapacağını tam, bizim ne yapacağımızı modelle tahmin eder; aramanın kararlarını tam tahmin edemediği için ona karşı kayıp, modelin tek başına oynadığı duruma göre çok daha küçüktür ve bu sayı o yüzden iyimserdir. Bütün rakipler bottur; insanlara karşı ölçüm yok.

**Model eklenirken yapılan karşılaştırma** (`duel`: oyuncu üç ayarlı bota karşı oturur; değer, aynı koltuktaki ayarlı bota göre el başına puan kazancıdır):

| Oyuncu | Tekli | Eşli (takım farkı) |
|---|---|---|
| Düz bot | −3,0 ± 0,6 | |
| Arama, 80 simülasyon | +20,3 ± 1,9 | +29,5 ± 6,8 |
| Arama, 320 simülasyon | aramadan +4,0 ± 2,9 fazla | |
| Model tek başına, anlık | +21,2 ± 0,6 | +38,7 ± 1,5 |
| **Arama 80 + sonraki turları model oynuyor** | **aramadan +15,8 ± 2,9 fazla** | **aramadan +17,3 ± 7,6 fazla** |
| Rakip ellerini gören arama (hile) | aramadan +47,9 ± 2,6 fazla | |
| Elleri ve deste sırasını gören (hile) | +78,5 ± 1,7 | |

Rakibin elini görmek 48 puan değerinde, ama açık bilgilerden bunun çok azı çıkarılabiliyor; kazanılabilen kısım "ileride iyi oynayacağım" varsayımından geliyor. Eşli satırlar 1500 elle ölçüldü ve gürültülü.

**Lig** (model eklenmeden önceki ölçüm): 700 el, karar başına 80 simülasyon, düz bot 1000 Elo kabul edilerek.

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

- Bütün ölçümler botlara karşı yapıldı; model de botlarla ve kendisiyle oynayarak öğrendi. İnsanlara karşı ölçülmedi.
- Eşli ağ, eşin ayarlı bot gibi oynadığı oyunlardan öğrendi.
- Model katlamalı ya da değiştirilmiş kurallarda ve maçın son iki elinde devreye girmez; o zaman senin sonraki turlarını da bot oynar.
- Model açıkken bir senaryo, botla oynatılana göre birkaç kat pahalıdır; bir tur sonra kesmek bu maliyeti yaklaşık yarıya indirdi.
- Eşli ağ hâlâ 3. tur yöntemiyle eğitilmiş olandır; kesilmiş simülasyon onunla da çalışır ve ölçüldü, ama ağın kendisi yeni yöntemle yeniden eğitilmedi.
- Simülasyonda rakipler bitmeye oynayan botlardır: açabildikleri an açarlar, çiftlerini çift açana işlemek için saklarlar. Blöfü ve masadaki oyuncunun kişisel tarzını bilmezler.
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
  features.rs, net.rs, model.rs modelin girdileri, ağın hesabı, model oyuncusu
crates/app      Leptos arayüzü ve analizi yapan web worker'lar (WASM)
  model/value.bin, partners.bin uygulamaya gömülen tekli ve eşli ağ
train/train.py  ağı eğiten betik (PyTorch)
dist            derlenmiş uygulama
```

## Geliştirme

Gerekenler: Rust, `wasm32-unknown-unknown` hedefi, [trunk](https://trunkrs.dev).

```sh
cd crates/app
trunk serve            # http://127.0.0.1:8080, değişiklikte yeniden derler
trunk build --release  # ../../dist
```

Windows'ta `baslat.bat` derlenmiş `dist` klasörünü `http://localhost:8101` adresinde açar (Python gerekir). IIS için gereken `web.config` derlemede `dist` içine kopyalanır.

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
| `duel <el> <rakipler> <oyuncu>...` | Oyuncuları aynı ellerde üç bota karşı karşılaştırır |
| `versus <el> <ağ> <rakipler> <oyuncu>...` | Aynısı, rakip olarak model, arama ya da her şeyi gören oyuncu da oturtulabilir; eşli oyun, maç kazanma oranı, el başına süre |
| `gen_data <klasör> <el> ...` | Model için eğitim verisi üretir |
| `net_check <ağ>` | Rust'ın ağı eğitim betiğiyle aynı hesapladığını doğrular |
| `tune [tur] [deneme] [el]` | Bot ayarlarını kendi kendine oynatarak iyileştirir |
| `gen_stats` | Hamle üreticinin bulduğu hamle sayıları ve süresi |
| `settle [pozisyon] [el]` | "Kesinleşince dur" kuralının uzun hesaba göre kaç puan kaybettirdiği |
| `season <maç> <ağ> ...` | Aynı ellerde tam maçlar: puana oynamak ile maçı kazanmaya oynamak arasındaki fark |

### Modeli yeniden eğitmek

Ekran kartı gerekmez ama hızlandırır. Bir kez:

```sh
python -m venv .venv
.venv\Scripts\pip install numpy torch --index-url https://download.pytorch.org/whl/cu130 --extra-index-url https://pypi.org/simple
```

Her tur yaklaşık 100 000 el veri üretir (16 çekirdekte 15–60 dakika) ve bir iki dakikada eğitir:

```sh
# 1. tur: seçeneklerden sonrasını bot oynar
cargo run --release -p okey-engine --example gen_data -- data/gen0 100000 31 - 50 6 10 8
.venv\Scripts\python train/train.py --data data/gen0 --out data/gen0.bin --plain

# 2. ve sonraki turlar: sonrasını bir önceki model oynar
cargo run --release -p okey-engine --example gen_data -- data/gen1 100000 51 data/gen0.bin 50 6 10 8 1 5
.venv\Scripts\python train/train.py --data data/gen1 --out data/gen1.bin --plain

cargo run --release -p okey-engine --example net_check -- data/gen1.bin
cargo run --release -p okey-engine --example duel -- 40000 tuned tuned model:data/gen1.bin
```

Eşli ağ için `gen_data` satırlarının sonuna `1` eklenir (ilk turda `... - 50 6 10 8 1 5 1`), `duel` içinde rakipler `tuned,partners` yazılır.

Dördüncü turdaki gibi keserek veri üretmek için sona iki sayı daha eklenir: oyuncunun kaç turu oynandıktan sonra modelin değeri alınacağı ve model koltuklarını devamında da modelin oynayıp oynamayacağı. Bu tur 24 deste sırasıyla yaklaşık iki saat sürer:

```sh
cargo run --release -p okey-engine --example gen_data -- data/g4 100000 71 data/gen2.bin 50 8 10 24 1 5 0 1 1
```

`--plain` olmadan eğitilen ağ "hangi taş gelirse açarım" girdilerini de okur; tek başına 1–2 puan daha iyi oynar ama simülasyonların içinde kullanmak için fazla yavaştır. Uygulamaya konacak ağ `crates/app/model/value.bin` (tekli) ya da `partners.bin` (eşli) üzerine kopyalanır.
