
// src/main.rs
use anyhow::{Context, Result};
use std::path::PathBuf;
use std::process::Command;

fn scan_document(output_path: &str) -> Result<PathBuf> {
    let ps_script = format!(r#"
        Add-Type -AssemblyName System.Runtime.InteropServices
        $deviceManager = New-Object -ComObject WIA.DeviceManager

        # Fujitsu fi-8150U tarayıcısını bul
        $allScanners = $deviceManager.DeviceInfos | Where-Object {{ $_.Type -eq 1 }}

        # Önce tüm tarayıcıları listele (debug için)
        Write-Host "Bulunan tarayıcılar:"
        foreach ($s in $allScanners) {{
            Write-Host "  - $($s.Properties('Name').Value)"
        }}

        # Fujitsu tarayıcısını seç
        $device = $allScanners | Where-Object {{
            $_.Properties('Name').Value -like '*Fujitsu*' -or
            $_.Properties('Name').Value -like '*fi-8150*'
        }} | Select-Object -First 1

        if (-not $device) {{
            throw "Fujitsu fi-8150U tarayıcısı bulunamadı! Lütfen USB bağlantısını kontrol edin."
        }}

        Write-Host "Seçilen tarayıcı: $($device.Properties('Name').Value)"

        $scanner = $device.Connect()
        $item = $scanner.Items[1]

        # Tarama ayarları (300 DPI, renkli)
        $item.Properties("6146").Value = 1   # Renkli
        $item.Properties("6147").Value = 300  # Yatay DPI
        $item.Properties("6148").Value = 300  # Dikey DPI

        # Taramayı başlat (WIA varsayılan format - genellikle BMP)
        $img = $item.Transfer()

        # Geçici BMP dosyası olarak kaydet
        $tempBmp = "{output_path}.bmp"
        if (Test-Path $tempBmp) {{ Remove-Item $tempBmp }}
        $img.SaveFile($tempBmp)

        Write-Output "OK:$tempBmp"
    "#);

    let output = Command::new("powershell")
        .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-Command", &ps_script])
        .output()
        .context("PowerShell çalıştırılamadı")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("Tarama hatası: {}", stderr);
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    println!("Tarama sonucu: {}", stdout.trim());

    Ok(PathBuf::from(format!("{}.bmp", output_path)))
}

fn main() -> Result<()> {
    println!("Fujitsu fi-8150 Tarayıcı");
    println!("========================");
    println!("Tarama başlatılıyor...\n");

    let output_path = "taranan_belge.png";
    let temp_bmp = scan_document(output_path)?;

    println!("✓ Tarama tamamlandı (BMP): {}", temp_bmp.display());

    // BMP'yi PNG'ye dönüştür
    let img = image::open(&temp_bmp)?;
    println!("  Boyut: {}x{}", img.width(), img.height());

    // PNG olarak kaydet
    img.save(output_path)?;
    println!("✓ PNG olarak kaydedildi: {}", output_path);

    // Geçici BMP dosyasını sil
    std::fs::remove_file(&temp_bmp)?;

    Ok(())
}