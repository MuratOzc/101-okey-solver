# Ayrıntılar

## Masada kullanım

Ekran ikiye ayrılır: masa (rakipler, son attıkları taşlar, açılan perler, istakan) ve o an sorulan tek soruyu soran panel. Telefon yan tutulunca panel sağda, dik tutulunca altta durur.

1. **Gösterge**, **kim başlıyor** ve **taşların** sorulur. Taşlar taş seçiciden tek dokunuşla girilir; bir taşın son kopyası kaldığında altında nokta çıkar, kalmadığında soluklaşır. Yanlış girdiğin taşa yeşil alanda dokunursan silinir.
2. Sıra sendeyken panel **öneriyi** gösterir; aynı hamle istakanın üstüne de çizilir: aynı renkle çevrili taşlar birlikte indirilir, numaralar sırayı verir, atılacak taş yukarı kalkar. Yaptıysan "Yaptım"a bas; başka bir şey yaptıysan alttaki seçeneklerden birine dokun ya da istakadan kendin seç.
3. Çekme sırası sendeyken panel desteden çekmeyle yandan almayı karşılaştırır ve **destenin son taşının kime gideceğini** gösterir.
4. Sıra rakipteyken istakanın yerine taş seçici gelir: önce desteden mi çekti yandan mı aldı, sonra attığı taş. Per indirdiyse taşlarını sırayla girersin (okeyi serideki yerinde gir: önce girersen alt uç, sonra girersen üst uç olur; göstergeyle çift: göstergenin eşine ve yanındaki taşa dokun); işlediyse masada o pere dokunursun.

Önerinin altında beklenen puan farkı, bitirme ihtimali ve attığın taşı sağdakinin alma ihtimali yazar. Program düşündükçe sayılar netleşir; beklemek zorunda değilsin.

Her rakibin adının altında **tahmini eli** yazar: açmamışsa en iyi perinin %80 ihtimalle düştüğü aralık, çift sayısı ve açma ihtimali ("açar %20 · sonuna %45": sıra sana gelmeden %20, el bitene kadar %45); açmışsa okey sayılmadan elindeki puan aralığı ve aynı biçimde bitirme ihtimali. Bunlar senin görebildiğin taşlardan ve rakiplerin yaptıklarından çıkarılan tahminlerdir; rakibin tarzını bilmez.

Üst çubuktaki ☷ düğmesi atılan taşları, elinin durumunu, rakiplerin tahmini elinin ayrıntısını ve görülmemiş taşları açar; ⚙ ayarları ve maç puanını. "Bu durumu kopyala" o anki durumu metin olarak panoya alır; saçma bir öneri görürsen bunu saklamak hatayı yeniden üretmeye yeter.

### El kayıtları

Uygulama her eli kendiliğinden kaydeder: girilen her hamle, o anda önerilen hamleler ve rakiplerin tahmini eli. El bitip yeni ele geçilince kayıt telefona eklenir; puanları yazdıysan onlar da kayda girer. Ayarlar → "Kayıtlı eller" bölümünden hepsini tek dosya olarak indirebilir, kopyalayabilir ya da silebilirsin. Tarayıcı verilerini silmek kayıtları da siler.

Geri alma geçmişi de telefonda tutulur: sayfayı yenilesen de son girişleri geri alabilirsin.

### Maç puanı

El bitince program puan tablosunu açar. Senin puanın elinden hesaplanır; açmayan rakiplere 101 yazılır, açanlar için elinde kalan puanı girersin (çift açanınki iki katı alınır). El boyunca yazılan cezalar ve bitirenin katları kendiliğinden eklenir. "Yeni el" puanları maç toplamına ekler; üst çubukta el numarası ve liderle aradaki fark görünür. Toplamlar ayarlardan düzeltilebilir, yeni maç da oradan başlar.

Program maç boyunca farkı açmaya oynar. Maçın son iki elinde hedefi **maçı kazanmak** olur: gerideyken risk alır, öndeyken garantiye oynar. O ellerde öneride puan farkı yerine "Maçı kazanma ihtimali %X" görünür.

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

1. **Bütün yasal hamleler üretilir:** açma, çift açma, işleme, okey alma ve atılacak taşın her birleşimi.
2. **Bilinmeyen taşlar rastgele dağıtılır.** Rakiplerin eli ve deste görülmemiş taşlardan çekilir; rakiplerin attığı taşlar ve açmamış olmaları ipucu olarak kullanılır.
3. **Her hamle aynı dağıtımlarda oynatılır.** Rakipleri botlar, senin sonraki turunu eğitilmiş bir yapay zekâ oynar. Bütün hamleler aynı dağıtımları gördüğü için aradaki fark şansa değil hamleye aittir.
4. **Açıkça geride kalan hamleler elenir**, kalan süre yakın olanlara harcanır.
5. **Sıralama** el modunda ortalama puan farkına, maçın son iki elinde maçı kazanma ihtimaline göre yapılır.

## Bilinen sınırlar

- Simülasyonda rakipler bottur: blöfü ve masadaki oyuncunun kişisel tarzını bilmezler.
- Program bugüne kadar yalnızca botlara karşı sınandı.
- Yapay zekâ katlamalı ya da değiştirilmiş kurallarda ve maçın son iki elinde devreye girmez; o zaman senin sonraki turlarını da bot oynar.

## Geliştirme

Gerekenler: Rust, `wasm32-unknown-unknown` hedefi, [trunk](https://trunkrs.dev).

```sh
cd crates/app
trunk serve            # http://127.0.0.1:8080
trunk build --release  # ../../dist
cargo test --release -p okey-engine
cargo test -p okey-app
```

Windows'ta `baslat.bat` derlenmiş `dist` klasörünü `http://localhost:8101` adresinde açar.

```
crates/engine   oyun motoru: kurallar, perler, botlar, hamle üretici, analiz, yapay zekâ
crates/app      arayüz ve analizi yapan web worker'lar
train/train.py  yapay zekâyı eğiten betik
```

`tests/rules.rs` her masa kuralı için bir test içerir. Ölçüm ve eğitim araçları `crates/engine/examples` altındadır.
