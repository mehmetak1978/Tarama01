# Tarama01 - Fujitsu fi-8150U Tarama API

Fujitsu fi-8150U belge tarayıcısını uzaktan kontrol etmek için geliştirilmiş bir REST API servisidir. Windows WIA (Windows Image Acquisition) arayüzü üzerinden PowerShell ile tarayıcıya komut gönderir ve taranan belgeleri Base64-encoded JPEG formatında istemcilere sunar.

## Özellikler

- **Simplex / Duplex tarama** - Tek yüz veya çift yüz (ön + arka) tarama desteği
- **Otomatik belge besleyici (ADF)** - Çok sayfalı belgeleri otomatik olarak sırayla tarar
- **Otomatik kırpma (auto-crop)** - Beyaz kenarları algılayıp otomatik olarak kırpar
- **5 tarama profili** - Farklı kullanım senaryolarına uygun hazır profiller
- **JPEG kalite ayarı** - 1-100 arası ayarlanabilir kalite
- **CORS desteği** - React vb. frontend uygulamalardan doğrudan erişim
- **Yapılandırılmış loglama** - `tracing` ile detaylı log çıktısı

## Gereksinimler

- Windows 10/11
- Rust 1.70+ (2021 edition)
- Fujitsu fi-8150U tarayıcı (WIA sürücüsü yüklü olmalı)
- PowerShell 5.1+

## Kurulum ve Çalıştırma

```bash
# Projeyi derle
cargo build --release

# Çalıştır (varsayılan: 0.0.0.0:3000)
cargo run --release

# Log seviyesini ayarla (isteğe bağlı)
RUST_LOG=debug cargo run --release
```

## API Endpoint'leri

### `GET /` ve `GET /health` - Sağlık Kontrolu

Tarayıcı servisinin çalışıp çalışmadığını kontrol eder.

**Yanit:**
```json
{
  "status": "ok",
  "scanner": "Fujitsu fi-8150U"
}
```

### `POST /scan` - Tarama Başlat

Belge taraması başlatır ve sonuçları döndürür.

**İstek Gövdesi (JSON):**

| Alan        | Tip      | Varsayılan | Açıklama                                              |
|-------------|----------|------------|-------------------------------------------------------|
| `duplex`    | bool     | `false`    | Çift yüz tarama                                       |
| `profile`   | string   | `"renkli"` | Tarama profili                                         |
| `auto_crop` | bool     | `true`     | Otomatik beyaz kenar kırpma                            |
| `pages`     | u32/null | `null`     | Taranacak yaprak sayısı (null ise otomatik algılama)   |
| `quality`   | u8       | `85`       | JPEG kalite (1-100)                                    |

**Örnek İstek:**
```bash
# Varsayılan ayarlarla tarama
curl -X POST http://localhost:3000/scan

# Duplex, yüksek kalite, 3 yaprak
curl -X POST http://localhost:3000/scan \
  -H "Content-Type: application/json" \
  -d '{"duplex": true, "profile": "yuksek", "pages": 3, "quality": 95}'
```

**Başarılı Yanıt:**
```json
{
  "success": true,
  "images": [
    {
      "image": "<base64-encoded JPEG>",
      "width": 2480,
      "height": 3508,
      "dpi": 300,
      "width_mm": 210.0,
      "height_mm": 297.0,
      "page": 1
    }
  ],
  "format": "jpeg",
  "duplex": false,
  "profile": "Renkli Belge",
  "auto_crop": true,
  "page_count": 1,
  "error": null
}
```

## Tarama Profilleri

| Profil         | Renk Modu  | DPI | Kullanım Alanı              |
|----------------|------------|-----|------------------------------|
| `hizli`        | Gri tonlama| 150 | Hızlı ön izleme             |
| `standart`     | Gri tonlama| 300 | Standart belge tarama        |
| `renkli`       | Renkli     | 300 | Renkli belge tarama (varsayılan) |
| `yuksek`       | Renkli     | 600 | Yüksek kaliteli tarama       |
| `siyah-beyaz`  | Siyah/Beyaz| 300 | Metin ağırlıklı belgeler     |

## Teknik Detaylar

- **Framework:** Axum 0.7 (async HTTP)
- **Tarayıcı İletişimi:** WIA COM nesneleri (PowerShell üzerinden)
- **Görüntü İşleme:** `image` crate ile BMP -> JPEG dönüşümü ve auto-crop
- **Tarama Timeout:** Yaprak başına 60 saniye
- **Yapraklar arası bekleme:** 2 saniye (tarayıcı serbest kalma süresi)
- **Bağlantı denemesi:** Tarayıcıya 3 deneme ile bağlanma
- **Dinleme adresi:** `0.0.0.0:3000`

## Bağımlılıklar

| Crate              | Kullanım Amacı                     |
|--------------------|-------------------------------------|
| `axum`             | HTTP web framework                  |
| `tokio`            | Async runtime                       |
| `tower-http`       | CORS ve HTTP trace middleware       |
| `image`            | Görüntü işleme (BMP okuma, JPEG yazma, kırpma) |
| `base64`           | Görüntülerin Base64 kodlaması       |
| `serde` / `serde_json` | JSON serileştirme              |
| `anyhow`           | Hata yönetimi                       |
| `tracing`          | Yapılandırılmış loglama             |

## Lisans

Bu proje kişisel/kurumsal kullanım amaçlıdır.
