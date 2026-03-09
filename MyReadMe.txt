v2: Çalışan bir versiyon. Rest API dinleyip, image bilgisi dönüyor.
v3: Çift taraflı tarama (duplex) desteği eklendi.
v4:
    tarama profili ve crop özelliği eklendi.
v5:
    birden fazla sayfayı arka arkaya tarama
(ÇALIŞIR VERSİYON)v6: Dosya boyu. jpeg'e çevrildi. Kalite değişkeni eklendi.
v7:
    Telemetri eklendi.
    │ İleride OTLP Ekleme (bilgi amaçlı)                                                                                                                                                                                              │
    │ Konsol çıktısından Grafana/Jaeger'a geçmek için ileride sadece şunlar eklenir:                                                                                                                                                  │
    │ - opentelemetry, opentelemetry_sdk, opentelemetry-otlp, tracing-opentelemetry crate'leri                                                                                                                                        │
    │ - main()'de mevcut subscriber'a bir OTLP layer eklenir                                                                                                                                                                          │
    │ - Mevcut #[instrument] ve tracing::info!() çağrıları hiç değişmez
  - Subscriber init in main() with EnvFilter (defaults to info, configurable via RUST_LOG)
  - TraceLayer::new_for_http() added to the Axum router — automatic spans for every HTTP request (method, path, status, duration)
  - #[tracing::instrument] on 4 functions: scan_endpoint, scan_document, scan_single_sheet, auto_crop — with skip for large data
  - All println!/eprintln! converted to tracing::info!, tracing::debug!, tracing::warn!, tracing::error! with structured fields
        Tracing Kullanım Rehberi

          Log Seviyesi Kontrolü

          Uygulamayı çalıştırırken RUST_LOG ortam değişkeni ile log seviyesini ayarlayabilirsin:

          - RUST_LOG=debug — auto-crop detayları, tarayıcı bekleme mesajları dahil her şeyi gösterir
          - RUST_LOG=info — varsayılan, normal akış bilgileri (tarama başladı, bitti, kaç sayfa vs.)
          - RUST_LOG=warn — sadece uyarı ve hatalar
          - RUST_LOG=error — sadece hatalar

          Modül bazlı filtreleme de yapabilirsin: RUST_LOG=tarama01=debug,tower_http=info gibi. Bu sayede kendi kodundan debug çıktısı alırken HTTP trace'lerini info seviyesinde tutabilirsin.

          Konsol Çıktısında Ne Göreceksin

          - Her HTTP isteği için otomatik satır: method, path, status kodu, süre (TraceLayer sayesinde)
          - Yapılandırılmış alanlar: profile=, duplex=, pages=, sheet= gibi key-value çiftleri — düz metin yerine aranabilir/filtrelenebilir bilgi
          - Span hiyerarşisi: scan_endpoint → scan_document → scan_single_sheet → auto_crop şeklinde iç içe span'lar, hangi işlemin hangi üst işlemin parçası olduğunu gösterir

          İleride OTLP'ye Geçiş

          Mevcut yapı buna hazır. Konsol çıktısından Jaeger/Grafana Tempo'ya geçmek istersen:
          1. opentelemetry, opentelemetry-otlp, tracing-opentelemetry crate'lerini eklersin
          2. main()'deki subscriber'a bir OTLP layer eklersin
          3. Kodda hiçbir şey değişmez — tüm tracing::info!() ve #[instrument] çağrıları aynen kalır

          Yani şu an konsola yazdığın her span/event, ileride doğrudan distributed tracing backend'ine akar.


PowerShell'de curl farklı çalışıyor. Şu komutu kullanın:

  Tek taraflı tarama:
  Invoke-WebRequest -Uri http://localhost:3000/scan -Method POST

  Çift taraflı (duplex) tarama:
  Invoke-WebRequest -Uri http://localhost:3000/scan -Method POST -ContentType "application/json" -Body '{"duplex": true}'

  Veya kısa versiyonu:

  irm http://localhost:3000/scan -Method POST
  irm http://localhost:3000/scan -Method POST -ContentType "application/json" -Body '{"duplex": true}'

  Sadece sağlık kontrolü için (GET):

  irm http://localhost:3000/health


  Debug build (hızlı derleme, yavaş çalışma):
    cargo build
    Çıktı: target\debug\Tarama01.exe

    Release build (yavaş derleme, hızlı çalışma — production için):
    cargo build --release
    Çıktı: target\release\Tarama01.exe

    Derleyip hemen çalıştırmak isterseniz:
    cargo run --release

ÇALIŞTIRMA
    PowerShell:
    $env:RUST_LOG="debug"; cargo run

    CMD:
    set RUST_LOG=debug && cargo run

    Kalıcı yapmak istersen Windows ortam değişkenlerine ekleyebilirsin:
    - Ayarlar → Sistem → Gelişmiş sistem ayarları → Ortam Değişkenleri → Yeni
    - Değişken adı: RUST_LOG, Değer: info (veya istediğin seviye)

    Belirtmezsen varsayılan olarak info seviyesinde çalışır.