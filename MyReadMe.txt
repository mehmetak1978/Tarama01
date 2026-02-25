PowerShell'de curl farklı çalışıyor. Şu komutu kullanın:

  Invoke-WebRequest -Uri http://localhost:3000/scan -Method POST

  Veya kısa versiyonu:

  irm http://localhost:3000/scan -Method POST

  Sadece sağlık kontrolü için (GET):

  irm http://localhost:3000/health