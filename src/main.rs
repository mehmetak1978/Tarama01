use anyhow::{Context, Result};
use axum::{
    http::StatusCode,
    routing::{get, post},
    Json, Router,
};
use base64::{engine::general_purpose::STANDARD, Engine};
use image::DynamicImage;
use serde::{Deserialize, Serialize};
use std::io::Cursor;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};
use tower_http::cors::{Any, CorsLayer};

#[derive(Deserialize)]
struct ScanRequest {
    #[serde(default)]
    duplex: bool,
    #[serde(default = "default_profile")]
    profile: String,
    #[serde(default = "default_true")]
    auto_crop: bool,
    /// Taranacak sayfa sayısı (belirtilmezse otomatik algılama)
    pages: Option<u32>,
}

fn default_profile() -> String {
    "renkli".to_string()
}

fn default_true() -> bool {
    true
}

struct ScanProfile {
    color_mode: u32, // WIA 6146: 1=Renkli, 2=Gri, 4=S/B
    dpi: u32,
    name: String,
}

fn get_profile(profile: &str) -> Result<ScanProfile> {
    match profile {
        "hizli" => Ok(ScanProfile {
            color_mode: 2,
            dpi: 150,
            name: "Hızlı Tarama".to_string(),
        }),
        "standart" => Ok(ScanProfile {
            color_mode: 2,
            dpi: 300,
            name: "Standart Belge".to_string(),
        }),
        "renkli" => Ok(ScanProfile {
            color_mode: 1,
            dpi: 300,
            name: "Renkli Belge".to_string(),
        }),
        "yuksek" => Ok(ScanProfile {
            color_mode: 1,
            dpi: 600,
            name: "Yüksek Kalite".to_string(),
        }),
        "siyah-beyaz" => Ok(ScanProfile {
            color_mode: 4,
            dpi: 300,
            name: "Siyah-Beyaz".to_string(),
        }),
        _ => anyhow::bail!(
            "Geçersiz profil: '{}'. Geçerli profiller: hizli, standart, renkli, yuksek, siyah-beyaz",
            profile
        ),
    }
}

#[derive(Serialize)]
struct ScannedPage {
    image: String,
    width: u32,
    height: u32,
    dpi: u32,
    width_mm: f64,
    height_mm: f64,
    page: u32,
}

#[derive(Serialize)]
struct ScanResponse {
    success: bool,
    images: Option<Vec<ScannedPage>>,
    format: Option<String>,
    duplex: bool,
    profile: Option<String>,
    auto_crop: bool,
    page_count: Option<u32>,
    error: Option<String>,
}

#[derive(Serialize)]
struct HealthResponse {
    status: String,
    scanner: String,
}

/// Beyaz kenarları tespit edip görüntüyü kırpar (auto-crop).
/// Eşik değeri (threshold) ile beyaza yakın pikseller de beyaz sayılır.
fn auto_crop(img: &DynamicImage) -> DynamicImage {
    let rgba = img.to_rgba8();
    let (w, h) = rgba.dimensions();
    let threshold: u8 = 245; // Bu değerin üstündeki R,G,B beyaz sayılır

    // Üstten: ilk beyaz olmayan satırı bul
    let mut top = 0u32;
    'top: for y in 0..h {
        for x in 0..w {
            let p = rgba.get_pixel(x, y);
            if p[0] < threshold || p[1] < threshold || p[2] < threshold {
                top = y;
                break 'top;
            }
        }
    }

    // Alttan: son beyaz olmayan satırı bul
    let mut bottom = h.saturating_sub(1);
    'bottom: for y in (0..h).rev() {
        for x in 0..w {
            let p = rgba.get_pixel(x, y);
            if p[0] < threshold || p[1] < threshold || p[2] < threshold {
                bottom = y;
                break 'bottom;
            }
        }
    }

    // Soldan: ilk beyaz olmayan sütunu bul
    let mut left = 0u32;
    'left: for x in 0..w {
        for y in top..=bottom {
            let p = rgba.get_pixel(x, y);
            if p[0] < threshold || p[1] < threshold || p[2] < threshold {
                left = x;
                break 'left;
            }
        }
    }

    // Sağdan: son beyaz olmayan sütunu bul
    let mut right = w.saturating_sub(1);
    'right: for x in (0..w).rev() {
        for y in top..=bottom {
            let p = rgba.get_pixel(x, y);
            if p[0] < threshold || p[1] < threshold || p[2] < threshold {
                right = x;
                break 'right;
            }
        }
    }

    // Güvenlik: en az 1x1 piksel olsun
    if right <= left || bottom <= top {
        return img.clone();
    }

    let crop_w = right - left + 1;
    let crop_h = bottom - top + 1;

    println!(
        "Auto-crop: {}x{} -> {}x{} (sol:{}, üst:{}, sağ:{}, alt:{})",
        w, h, crop_w, crop_h, left, top, right, bottom
    );

    img.crop_imm(left, top, crop_w, crop_h)
}

/// Tek bir sayfa tarar. Başarılıysa BMP dosya yolunu döndürür.
/// Besleyici boşsa veya hata varsa None döndürür.
fn scan_single_page(
    page_num: u32,
    profile: &ScanProfile,
    duplex: bool,
    temp_dir: &std::path::Path,
) -> Result<Option<std::path::PathBuf>> {
    let bmp_path = temp_dir.join(format!("scan_page_{}.bmp", page_num));
    let bmp_path_str = bmp_path.to_string_lossy();

    let duplex_prop = if duplex { "5" } else { "1" };

    let ps_script = format!(
        r#"
        $ErrorActionPreference = 'Stop'
        $deviceManager = New-Object -ComObject WIA.DeviceManager
        $device = $deviceManager.DeviceInfos | Where-Object {{ $_.Type -eq 1 }} | Where-Object {{
            $_.Properties('Name').Value -like '*Fujitsu*' -or
            $_.Properties('Name').Value -like '*fi-8150*'
        }} | Select-Object -First 1

        if (-not $device) {{ throw "Tarayıcı bulunamadı!" }}

        # Bağlan (3 deneme)
        $scanner = $null
        for ($attempt = 1; $attempt -le 3; $attempt++) {{
            try {{
                $scanner = $device.Connect()
                break
            }} catch {{
                if ($attempt -eq 3) {{ throw "Tarayıcı meşgul: $_" }}
                Start-Sleep -Seconds 2
            }}
        }}

        # Besleyici ayarı
        try {{ $scanner.Properties("3088").Value = {duplex_prop} }} catch {{}}
        # Tek sayfa tara
        $scanner.Properties("3096").Value = 1

        $item = $scanner.Items[1]
        $item.Properties("6146").Value = {color_mode}
        $item.Properties("6147").Value = {dpi}
        $item.Properties("6148").Value = {dpi}

        $img = $item.Transfer()
        $bmpFile = "{bmp_path_str}"
        if (Test-Path $bmpFile) {{ Remove-Item $bmpFile -Force }}
        $img.SaveFile($bmpFile)

        # COM nesnelerini temizle
        [System.Runtime.Interopservices.Marshal]::ReleaseComObject($img) | Out-Null
        [System.Runtime.Interopservices.Marshal]::ReleaseComObject($item) | Out-Null
        [System.Runtime.Interopservices.Marshal]::ReleaseComObject($scanner) | Out-Null
        [System.Runtime.Interopservices.Marshal]::ReleaseComObject($deviceManager) | Out-Null
        [System.GC]::Collect()
        [System.GC]::WaitForPendingFinalizers()

        Write-Output "OK"
    "#,
        duplex_prop = duplex_prop,
        color_mode = profile.color_mode,
        dpi = profile.dpi,
        bmp_path_str = bmp_path_str,
    );

    let mut child = Command::new("powershell")
        .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-Command", &ps_script])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("PowerShell çalıştırılamadı")?;

    // stdout/stderr thread'leri
    let stdout_pipe = child.stdout.take().unwrap();
    let stderr_pipe = child.stderr.take().unwrap();
    let stdout_handle = std::thread::spawn(move || {
        let mut buf = Vec::new();
        std::io::Read::read_to_end(&mut std::io::BufReader::new(stdout_pipe), &mut buf).ok();
        buf
    });
    let stderr_handle = std::thread::spawn(move || {
        let mut buf = Vec::new();
        std::io::Read::read_to_end(&mut std::io::BufReader::new(stderr_pipe), &mut buf).ok();
        buf
    });

    // 60 saniye timeout
    let timeout = Duration::from_secs(60);
    let start = Instant::now();
    let status = loop {
        match child.try_wait().context("PowerShell durumu kontrol edilemedi")? {
            Some(status) => break status,
            None => {
                if start.elapsed() > timeout {
                    let _ = child.kill();
                    let _ = child.wait();
                    // Timeout = besleyici boş olabilir
                    return Ok(None);
                }
                std::thread::sleep(Duration::from_millis(500));
            }
        }
    };

    let stdout_bytes = stdout_handle.join().unwrap_or_default();
    let stderr_bytes = stderr_handle.join().unwrap_or_default();

    if !status.success() {
        let stderr = String::from_utf8_lossy(&stderr_bytes);
        let stderr_str = stderr.to_string();
        // Besleyici boş veya kağıt yok hatalarını "bitti" olarak say
        if stderr_str.contains("paper")
            || stderr_str.contains("empty")
            || stderr_str.contains("no document")
            || stderr_str.contains("FEED")
        {
            return Ok(None);
        }
        // İlk sayfa değilse hata yerine None dön
        if page_num > 1 {
            println!("Sayfa {} hatası (tarama durduruluyor): {}", page_num, stderr_str.trim());
            return Ok(None);
        }
        anyhow::bail!("Tarama hatası: {}", stderr_str);
    }

    let stdout = String::from_utf8_lossy(&stdout_bytes);
    if stdout.trim() == "OK" && bmp_path.exists() {
        Ok(Some(bmp_path))
    } else {
        Ok(None)
    }
}

fn scan_document(duplex: bool, profile: &ScanProfile, do_auto_crop: bool, expected_pages: Option<u32>) -> Result<Vec<(Vec<u8>, u32, u32)>> {
    let temp_dir = std::env::temp_dir();
    let max_pages = expected_pages.unwrap_or(100);
    let mut pages = Vec::new();

    for i in 1..=max_pages {
        println!("Sayfa {} taranıyor...", i);

        match scan_single_page(i, profile, duplex, &temp_dir) {
            Ok(Some(bmp_path)) => {
                println!("Sayfa {} başarılı: {}", i, bmp_path.display());

                let img = image::open(&bmp_path)
                    .context(format!("Sayfa {} açılamadı", i))?;

                let img = if do_auto_crop {
                    auto_crop(&img)
                } else {
                    img
                };

                let width = img.width();
                let height = img.height();

                let mut cursor = Cursor::new(Vec::new());
                img.write_to(&mut cursor, image::ImageFormat::Png)
                    .context(format!("Sayfa {} PNG dönüşümü başarısız", i))?;

                pages.push((cursor.into_inner(), width, height));
                let _ = std::fs::remove_file(&bmp_path);

                // Sonraki sayfa için tarayıcının serbest kalmasını bekle
                if i < max_pages {
                    println!("Tarayıcı serbest bırakılıyor (2 saniye)...");
                    std::thread::sleep(Duration::from_secs(2));
                }
            }
            Ok(None) => {
                println!("Sayfa {} yok veya besleyici boş, tarama tamamlandı.", i);
                break;
            }
            Err(e) => {
                if pages.is_empty() {
                    return Err(e);
                }
                println!("Sayfa {} hatası: {}, mevcut sayfalarla devam ediliyor.", i, e);
                break;
            }
        }
    }

    if pages.is_empty() {
        anyhow::bail!("Hiç sayfa taranamadı!");
    }

    println!("Toplam {} sayfa tarandı.", pages.len());
    Ok(pages)
}

async fn health_check() -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok".to_string(),
        scanner: "Fujitsu fi-8150U".to_string(),
    })
}

async fn scan_endpoint(body: Option<Json<ScanRequest>>) -> (StatusCode, Json<ScanResponse>) {
    let (duplex, profile_key, do_auto_crop, expected_pages) = match body {
        Some(Json(r)) => (r.duplex, r.profile, r.auto_crop, r.pages),
        None => (false, default_profile(), true, None),
    };

    let profile = match get_profile(&profile_key) {
        Ok(p) => p,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(ScanResponse {
                    success: false,
                    images: None,
                    format: None,
                    duplex,
                    profile: Some(profile_key),
                    auto_crop: do_auto_crop,
                    page_count: None,
                    error: Some(e.to_string()),
                }),
            );
        }
    };

    println!(
        "Tarama isteği alındı (profil: {}, duplex: {}, auto_crop: {}, pages: {})...",
        profile.name,
        if duplex { "çift taraflı" } else { "tek taraflı" },
        do_auto_crop,
        expected_pages.map_or("otomatik".to_string(), |p| p.to_string())
    );

    let dpi = profile.dpi;
    let profile_name = profile.name.clone();

    match scan_document(duplex, &profile, do_auto_crop, expected_pages) {
        Ok(pages) => {
            let page_count = pages.len() as u32;
            let images: Vec<ScannedPage> = pages
                .into_iter()
                .enumerate()
                .map(|(i, (png_data, width, height))| ScannedPage {
                    image: STANDARD.encode(&png_data),
                    width,
                    height,
                    dpi,
                    width_mm: (width as f64 / dpi as f64) * 25.4,
                    height_mm: (height as f64 / dpi as f64) * 25.4,
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
                    profile: Some(profile_name),
                    auto_crop: do_auto_crop,
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
                    profile: Some(profile_name),
                    auto_crop: do_auto_crop,
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
    println!("                  Body: {{\"duplex\": bool, \"profile\": string, \"auto_crop\": bool, \"pages\": number}}");
    println!("\nProfiller: hizli, standart, renkli (varsayılan), yuksek, siyah-beyaz");
    println!();

    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
