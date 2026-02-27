v2: Çalışan bir versiyon. Rest API dinleyip, image bilgisi dönüyor.
v3: Çift taraflı tarama (duplex) desteği eklendi.
v4:
    tarama profili ve crop özelliği eklendi.
v5:
    birden fazla sayfayı arka arkaya tarama




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

  Ayrıca asıl sorunu da netleştireyim: Bu Fujitsu fi-8150U'nun WIA driver'ı çoklu sayfa taramayı desteklemiyor. İkinci Transfer() çağrısı her zaman asılı kalıyor. Fiziksel olarak 2 sayfa geçiyor ama WIA sadece 1 tanesini
  döndürüyor.

  Bunun için iki alternatif var:

  1. NAPS2 CLI — ücretsiz, Fujitsu ADF'yi mükemmel destekliyor, komut satırından tarama yapılabiliyor
  2. Windows.Devices.Scanners (UWP API) — modern Windows tarama API'si, feeder'ı doğal destekliyor

  NAPS2 kurulu mu bilgisayarında? Yoksa kurmak ister misin? En güvenilir çözüm bu olur.

