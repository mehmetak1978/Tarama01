use anyhow::{Context, Result};
use axum::{
    http::StatusCode,
    routing::{get, post},
    Json, Router,
};
use base64::{engine::general_purpose::STANDARD, Engine};
use serde::{Deserialize, Serialize};
use std::io::Cursor;
use std::process::Command;
use tower_http::cors::{Any, CorsLayer};

#[derive(Deserialize)]
struct ScanRequest {
    #[serde(default)]
    duplex: bool,
}

#[derive(Serialize)]
struct ScannedPage {
    image: String,
    width: u32,
    height: u32,
    page: u32,
}

#[derive(Serialize)]
struct ScanResponse {
    success: bool,
    images: Option<Vec<ScannedPage>>,
    format: Option<String>,
    duplex: bool,
    page_count: Option<u32>,
    error: Option<String>,
}

#[derive(Serialize)]
struct HealthResponse {
    status: String,
    scanner: String,
}

fn scan_document(duplex: bool) -> Result<Vec<(Vec<u8>, u32, u32)>> {
    let temp_dir = std::env::temp_dir();
    let temp_dir_str = temp_dir.to_string_lossy();

    let duplex_setup = if duplex {
        r#"
        # Çift taraflı tarama ayarı
        try {
            # Document Handling Select: FEEDER(1) + DUPLEX(4) = 5
            $scanner.Properties("3088").Value = 5
        } catch {
            throw "Çift taraflı tarama ayarı yapılamadı: $_"
        }
        "#
    } else {
        ""
    };

    let duplex_back_scan = if duplex {
        r#"
        # Arka yüz tarama
        try {
            $imgBack = $item.Transfer()
            $pageCount++
            $bmpFileBack = "$tempBase\scan_page_$pageCount.bmp"
            if (Test-Path $bmpFileBack) { Remove-Item $bmpFileBack -Force }
            $imgBack.SaveFile($bmpFileBack)
        } catch {
            # Arka yüz alınamadıysa tek sayfa ile devam et
            Write-Output "WARN:Arka yuz alinamadi, tek sayfa ile devam ediliyor"
        }
        "#
    } else {
        ""
    };

    let ps_script = format!(
        r#"
        $ErrorActionPreference = 'Stop'
        Add-Type -AssemblyName System.Runtime.InteropServices
        $deviceManager = New-Object -ComObject WIA.DeviceManager

        # Fujitsu fi-8150U tarayıcısını bul
        $allScanners = $deviceManager.DeviceInfos | Where-Object {{ $_.Type -eq 1 }}
        $device = $allScanners | Where-Object {{
            $_.Properties('Name').Value -like '*Fujitsu*' -or
            $_.Properties('Name').Value -like '*fi-8150*'
        }} | Select-Object -First 1

        if (-not $device) {{
            throw "Fujitsu fi-8150U tarayıcısı bulunamadı!"
        }}

        $scanner = $device.Connect()

        {duplex_setup}

        $item = $scanner.Items[1]

        # Tarama ayarları (300 DPI, renkli)
        $item.Properties("6146").Value = 1   # Renkli
        $item.Properties("6147").Value = 300  # Yatay DPI
        $item.Properties("6148").Value = 300  # Dikey DPI

        $pageCount = 0
        $tempBase = "{temp_dir_str}"

        # Ön yüz tarama
        $img = $item.Transfer()
        $pageCount++
        $bmpFile = "$tempBase\scan_page_$pageCount.bmp"
        if (Test-Path $bmpFile) {{ Remove-Item $bmpFile -Force }}
        $img.SaveFile($bmpFile)

        {duplex_back_scan}

        # PNG'ye dönüştür
        Add-Type -AssemblyName System.Drawing
        for ($i = 1; $i -le $pageCount; $i++) {{
            $bmp = "$tempBase\scan_page_$i.bmp"
            $png = "$tempBase\scan_page_$i.png"
            if (Test-Path $png) {{ Remove-Item $png -Force }}

            try {{
                $bitmap = [System.Drawing.Image]::FromFile($bmp)
                $bitmap.Save($png, [System.Drawing.Imaging.ImageFormat]::Png)
                $bitmap.Dispose()
            }} catch {{
                # PNG dönüşümü başarısız olursa BMP ile devam
            }}
        }}

        Write-Output "PAGES:$pageCount"
    "#,
        duplex_setup = duplex_setup,
        duplex_back_scan = duplex_back_scan,
        temp_dir_str = temp_dir_str,
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

    // Sayfa sayısını bul
    let page_count: u32 = stdout
        .lines()
        .find(|l| l.starts_with("PAGES:"))
        .and_then(|l| l.strip_prefix("PAGES:"))
        .and_then(|n| n.trim().parse().ok())
        .unwrap_or(1);

    println!("Taranan sayfa sayısı: {}", page_count);

    let mut pages = Vec::new();

    for i in 1..=page_count {
        let bmp_path = temp_dir.join(format!("scan_page_{}.bmp", i));
        let png_path = temp_dir.join(format!("scan_page_{}.png", i));

        // PNG varsa onu, yoksa BMP'yi kullan
        let (img_path, is_png) = if png_path.exists() {
            (png_path.clone(), true)
        } else if bmp_path.exists() {
            (bmp_path.clone(), false)
        } else {
            anyhow::bail!("Sayfa {} dosyası bulunamadı!", i);
        };

        println!("Sayfa {} okunuyor: {}", i, img_path.display());

        let img = image::open(&img_path)
            .context(format!("Sayfa {} görüntüsü açılamadı: {}", i, img_path.display()))?;
        let width = img.width();
        let height = img.height();

        let png_data = if is_png {
            std::fs::read(&img_path).context(format!("Sayfa {} PNG dosyası okunamadı", i))?
        } else {
            let mut cursor = Cursor::new(Vec::new());
            img.write_to(&mut cursor, image::ImageFormat::Png)
                .context(format!("Sayfa {} PNG dönüşümü başarısız", i))?;
            cursor.into_inner()
        };

        pages.push((png_data, width, height));

        // Geçici dosyaları sil
        let _ = std::fs::remove_file(&bmp_path);
        let _ = std::fs::remove_file(&png_path);
    }

    if pages.is_empty() {
        anyhow::bail!("Hiç sayfa taranamadı!");
    }

    Ok(pages)
}

async fn health_check() -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok".to_string(),
        scanner: "Fujitsu fi-8150U".to_string(),
    })
}

async fn scan_endpoint(body: Option<Json<ScanRequest>>) -> (StatusCode, Json<ScanResponse>) {
    let duplex = body.map(|Json(r)| r.duplex).unwrap_or(false);
    println!(
        "Tarama isteği alındı (duplex: {})...",
        if duplex { "çift taraflı" } else { "tek taraflı" }
    );

    match scan_document(duplex) {
        Ok(pages) => {
            let page_count = pages.len() as u32;
            let images: Vec<ScannedPage> = pages
                .into_iter()
                .enumerate()
                .map(|(i, (png_data, width, height))| ScannedPage {
                    image: STANDARD.encode(&png_data),
                    width,
                    height,
                    page: (i + 1) as u32,
                })
                .collect();

            println!("Tarama başarılı: {} sayfa", page_count);

            (
                StatusCode::OK,
                Json(ScanResponse {
                    success: true,
                    images: Some(images),
                    format: Some("png".to_string()),
                    duplex,
                    page_count: Some(page_count),
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
                    images: None,
                    format: None,
                    duplex,
                    page_count: None,
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
    println!("  POST /scan    - Tarama başlat");
    println!("                  Body: {{\"duplex\": true/false}}");
    println!();

    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
