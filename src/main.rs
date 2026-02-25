use anyhow::{Context, Result};
use axum::{
    http::StatusCode,
    routing::{get, post},
    Json, Router,
};
use base64::{engine::general_purpose::STANDARD, Engine};
use serde::Serialize;
use std::io::Cursor;
use std::process::Command;
use tower_http::cors::{Any, CorsLayer};

#[derive(Serialize)]
struct ScanResponse {
    success: bool,
    image: Option<String>,
    width: Option<u32>,
    height: Option<u32>,
    format: Option<String>,
    error: Option<String>,
}

#[derive(Serialize)]
struct HealthResponse {
    status: String,
    scanner: String,
}

fn scan_document() -> Result<(Vec<u8>, u32, u32)> {
    let temp_path = std::env::temp_dir().join("scan_temp.bmp");
    let temp_path_str = temp_path.to_string_lossy();

    let ps_script = format!(
        r#"
        $ErrorActionPreference = 'Stop'
        Add-Type -AssemblyName System.Runtime.InteropServices
        $deviceManager = New-Object -ComObject WIA.DeviceManager

        # Fujitsu fi-8150U tarayıcısını bul
        $allScanners = $deviceManager.DeviceInfos | Where-Object {{ $_.Type -eq 1 }}

        # Fujitsu tarayıcısını seç
        $device = $allScanners | Where-Object {{
            $_.Properties('Name').Value -like '*Fujitsu*' -or
            $_.Properties('Name').Value -like '*fi-8150*'
        }} | Select-Object -First 1

        if (-not $device) {{
            throw "Fujitsu fi-8150U tarayıcısı bulunamadı!"
        }}

        $scanner = $device.Connect()
        $item = $scanner.Items[1]

        # Tarama ayarları (300 DPI, renkli)
        $item.Properties("6146").Value = 1   # Renkli
        $item.Properties("6147").Value = 300  # Yatay DPI
        $item.Properties("6148").Value = 300  # Dikey DPI

        # Taramayı başlat
        $img = $item.Transfer()

        # Önce BMP olarak kaydet
        $bmpFile = "{temp_path_str}"
        if (Test-Path $bmpFile) {{ Remove-Item $bmpFile -Force }}
        $img.SaveFile($bmpFile)

        # Dosya oluştu mu kontrol et
        if (-not (Test-Path $bmpFile)) {{
            throw "Tarama dosyası oluşturulamadı: $bmpFile"
        }}

        $fileSize = (Get-Item $bmpFile).Length
        if ($fileSize -eq 0) {{
            throw "Tarama dosyası boş (0 byte)"
        }}

        # BMP'yi PNG'ye dönüştür (WIA ImageProcess ile)
        $pngFile = "{temp_path_str}.png"
        if (Test-Path $pngFile) {{ Remove-Item $pngFile -Force }}

        try {{
            Add-Type -AssemblyName System.Drawing
            $bitmap = [System.Drawing.Image]::FromFile($bmpFile)
            $bitmap.Save($pngFile, [System.Drawing.Imaging.ImageFormat]::Png)
            $bitmap.Dispose()
        }} catch {{
            # PNG dönüşümü başarısız olursa BMP ile devam et
            Write-Output "BMP:$fileSize"
            exit 0
        }}

        $pngSize = (Get-Item $pngFile).Length
        Write-Output "PNG:$pngSize"
    "#
    );

    let output = Command::new("powershell")
        .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-Command", &ps_script])
        .output()
        .context("PowerShell çalıştırılamadı")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("Tarama hatası: {}", stderr);
    }

    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    println!("PowerShell çıktısı: {}", stdout);

    let png_path = temp_path.with_extension("bmp.png");

    // PowerShell PNG dönüşümü başarılı olduysa PNG dosyasını kullan
    let (img_path, is_png) = if stdout.starts_with("PNG:") && png_path.exists() {
        println!("PNG dosyası kullanılıyor: {}", png_path.display());
        (png_path.clone(), true)
    } else if temp_path.exists() {
        println!("BMP dosyası kullanılıyor: {}", temp_path.display());
        (temp_path.clone(), false)
    } else {
        anyhow::bail!("Tarama dosyası bulunamadı! Beklenen konum: {}", temp_path.display());
    };

    let img = image::open(&img_path)
        .context(format!("Görüntü açılamadı: {}", img_path.display()))?;
    let width = img.width();
    let height = img.height();

    // PNG olarak memory'ye yaz
    let mut png_data = Cursor::new(Vec::new());
    if is_png {
        // Zaten PNG, doğrudan oku
        png_data = Cursor::new(std::fs::read(&img_path).context("PNG dosyası okunamadı")?);
    } else {
        img.write_to(&mut png_data, image::ImageFormat::Png)
            .context("PNG dönüşümü başarısız")?;
    }

    // Geçici dosyaları sil
    let _ = std::fs::remove_file(&temp_path);
    let _ = std::fs::remove_file(&png_path);

    Ok((png_data.into_inner(), width, height))
}

async fn health_check() -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok".to_string(),
        scanner: "Fujitsu fi-8150U".to_string(),
    })
}

async fn scan_endpoint() -> (StatusCode, Json<ScanResponse>) {
    println!("Tarama isteği alındı...");

    match scan_document() {
        Ok((png_data, width, height)) => {
            let base64_image = STANDARD.encode(&png_data);
            println!("Tarama başarılı: {}x{}", width, height);

            (
                StatusCode::OK,
                Json(ScanResponse {
                    success: true,
                    image: Some(base64_image),
                    width: Some(width),
                    height: Some(height),
                    format: Some("png".to_string()),
                    error: None,
                }),
            )
        }
        Err(e) => {
            eprintln!("Tarama hatası: {}", e);

            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ScanResponse {
                    success: false,
                    image: None,
                    width: None,
                    height: None,
                    format: None,
                    error: Some(e.to_string()),
                }),
            )
        }
    }
}

#[tokio::main]
async fn main() {
    println!("=================================");
    println!("  Fujitsu fi-8150U Tarama API");
    println!("=================================");

    // CORS ayarları - React'tan erişim için
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    let app = Router::new()
        .route("/", get(health_check))
        .route("/health", get(health_check))
        .route("/scan", post(scan_endpoint))
        .layer(cors);

    let addr = "0.0.0.0:3000";
    println!("\nAPI sunucusu başlatılıyor: http://{}", addr);
    println!("\nEndpoint'ler:");
    println!("  GET  /        - Sağlık kontrolü");
    println!("  GET  /health  - Sağlık kontrolü");
    println!("  POST /scan    - Tarama başlat\n");

    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
