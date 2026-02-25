PowerShell'de curl farklı çalışıyor. Şu komutu kullanın:

  Invoke-WebRequest -Uri http://localhost:3000/scan -Method POST

  Veya kısa versiyonu:

  irm http://localhost:3000/scan -Method POST

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