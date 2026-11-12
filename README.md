# 101 Okey Çözücü

Açayım mı, ne atayım, yandan alayım mı? 101 Okey'de her turun sorusuna, olasılık ve simülasyonla hesaplanmış bir cevap veren analiz aracı.

**Dene:** https://101.muratozcan.me

## Neler yapar

**Her karara öneri.** Açmak, hangi taşı hangi pere işlemek, ne atmak, desteden mi çekmek yandan mı almak: hepsini tek hamle olarak, adım adım yazar.

**Taş sayımı.** Atılan, açılan, işlenen her taşı takip eder; neyin kaldığını, bitirme ihtimalini ve attığın taşın alınma ihtimalini gösterir.

**Eşli oyun.** Takım puanına göre oynar; son taşı ortağına kaydırmak gibi hamleleri hesaba katar.

**Maç bilinci.** Kaçıncı elde ve kaç puan geride olduğunu girersen gerideyken risk alır, öndeyken garantiye oynar.

**Senin masanın kuralları.** Katlamalı, açma eşikleri, cezalar ve katlar ayarlanır.

**Her yerde.** Telefonda ana ekrana eklenir, internetsiz çalışır; girdiğin hiçbir şey cihazından çıkmaz.

## Nasıl kullanılır

El dağıtılınca göstergeyi, kimin başladığını ve taşlarını girersin. Sonra oyun boyunca masada ne oluyorsa onu işaretlersin: rakip çekti mi yandan mı aldı, ne açtı, ne attı. Sıra sana gelince program ne yapacağını yazar, mesela "Açma, Sarı 1 at" ya da "Bu üç peri aç, Mavi 7'yi soldakinin serisine işle, Kırmızı 2 at". Oynadıktan sonra onaylarsın, tur devam eder.

## Hile mi?

Hayır. Program rakibin elini ya da destenin sırasını bilmez; senin de görebildiğin taşları sayar. Farkı şuradan gelir: iyi bir oyuncunun kafasında yaptığını, yani olasılıkları tartmayı ve hamleleri sonuna kadar düşünmeyi, binlerce kez ve hatasız yapar. Bunu yaparken kendi kendine oynayarak eğitilmiş bir yapay zekâdan yardım alır.

## Kurallar

Varsayılan olarak:

- 101 puan perle ya da 5 çiftle açılır.
- Yandan alınan taş o tur kullanılmak zorundadır.
- İşlek taş ya da okey atmak 101 cezadır.
- Biten −101 alır; okey atarak, elden ya da çiftle bitmek puanı ikiye katlar.
- Eşli oyunda ortağın bitince takıma −202 yazılır.

Katlamalı dahil hepsi "Kurallar" ekranından değiştirilebilir.

## Kurulum

**Kurmadan kullan:** [101.muratozcan.me](https://101.muratozcan.me) adresini aç, istersen ana ekrana ekle.

**Kendin derle:** Rust, `wasm32-unknown-unknown` hedefi ve [trunk](https://trunkrs.dev) gerekir.

```sh
cd crates/app
trunk build --release
```

Çıkan `dist` klasörü herhangi bir statik sunucuda çalışır.

## Ayrıntılar

Kuralların tam listesi, ölçümler, proje yapısı ve yapay zekâyı yeniden eğitme adımları: [docs/ayrintilar.md](docs/ayrintilar.md)

## Lisans

[MIT](LICENSE)
