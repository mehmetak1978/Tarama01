v2: Çalışan bir versiyon. Rest API dinleyip, image bilgisi dönüyor.
v3: Çift taraflı tarama (duplex) desteği eklendi.
v4:
    tarama profili ve crop özelliği eklendi.





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


