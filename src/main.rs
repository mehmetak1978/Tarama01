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
use std::process::Command;
use tower_http::cors::{Any, CorsLayer};

#[derive(Deserialize)]
struct ScanRequest {
    #[serde(default)]
    duplex: bool,
    #[serde(default = "default_profile")]
    profile: String,
    #[serde(default = "default_true")]
    auto_crop: bool,
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

fn scan_document(duplex: bool, profile: &ScanProfile, do_auto_crop: bool) -> Result<Vec<(Vec<u8>, u32, u32)>> {
    let temp_dir = std::env::temp_dir();
    let temp_dir_str = temp_dir.to_string_lossy();
    let color_mode = profile.color_mode;
    let dpi = profile.dpi;

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

        # Tarayıcı meşgulse yeniden dene (3 deneme)
        $scanner = $null
        for ($attempt = 1; $attempt -le 3; $attempt++) {{
            try {{
                $scanner = $device.Connect()
                break
            }} catch {{
                if ($attempt -eq 3) {{ throw "Tarayıcı meşgul, 3 deneme başarısız: $_" }}
                Write-Host "Tarayıcı meşgul, $attempt. deneme başarısız. 2 saniye bekleniyor..."
                Start-Sleep -Seconds 2
            }}
        }}

        {duplex_setup}

        $item = $scanner.Items[1]

        # Tarama ayarları (profil: {profile_name})
        $item.Properties("6146").Value = {color_mode}  # Renk modu
        $item.Properties("6147").Value = {dpi}  # Yatay DPI
        $item.Properties("6148").Value = {dpi}  # Dikey DPI

        $pageCount = 0
        $tempBase = "{temp_dir_str}"
        $hasMorePages = $true

        # Besleyicide kağıt kalmayana kadar tara
        while ($hasMorePages) {{
            # Besleyicide kağıt var mı kontrol et (Property 3087, Bit 0 = FEED_READY)
            if ($pageCount -gt 0) {{
                try {{
                    $feedStatus = $scanner.Properties("3087").Value
                    if (-not ($feedStatus -band 1)) {{
                        $hasMorePages = $false
                        continue
                    }}
                }} catch {{
                    $hasMorePages = $false
                    continue
                }}
            }}

            try {{
                $img = $item.Transfer()
                $pageCount++
                $bmpFile = "$tempBase\scan_page_$pageCount.bmp"
                if (Test-Path $bmpFile) {{ Remove-Item $bmpFile -Force }}
                $img.SaveFile($bmpFile)
            }} catch {{
                # Besleyici boş veya başka hata - taramayı durdur
                if ($pageCount -eq 0) {{
                    throw "Tarama hatası: $_"
                }}
                $hasMorePages = $false
            }}
        }}

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
        temp_dir_str = temp_dir_str,
        color_mode = color_mode,
        dpi = dpi,
        profile_name = profile.name,
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
        let (img_path, _is_png) = if png_path.exists() {
            (png_path.clone(), true)
        } else if bmp_path.exists() {
            (bmp_path.clone(), false)
        } else {
            anyhow::bail!("Sayfa {} dosyası bulunamadı!", i);
        };

        println!("Sayfa {} okunuyor: {}", i, img_path.display());

        let img = image::open(&img_path)
            .context(format!("Sayfa {} görüntüsü açılamadı: {}", i, img_path.display()))?;

        // Auto-crop isteniyorsa beyaz kenarları kırp
        let img = if do_auto_crop {
            auto_crop(&img)
        } else {
            img
        };
        let width = img.width();
        let height = img.height();

        // Kırpılmış görüntüyü PNG olarak encode et
        let mut cursor = Cursor::new(Vec::new());
        img.write_to(&mut cursor, image::ImageFormat::Png)
            .context(format!("Sayfa {} PNG dönüşümü başarısız", i))?;
        let png_data = cursor.into_inner();

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
    let (duplex, profile_key, do_auto_crop) = match body {
        Some(Json(r)) => (r.duplex, r.profile, r.auto_crop),
        None => (false, default_profile(), true),
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
        "Tarama isteği alındı (profil: {}, duplex: {}, auto_crop: {})...",
        profile.name,
        if duplex { "çift taraflı" } else { "tek taraflı" },
        do_auto_crop
    );

    let dpi = profile.dpi;
    let profile_name = profile.name.clone();

    match scan_document(duplex, &profile, do_auto_crop) {
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
    println!("                  Body: {{\"duplex\": bool, \"profile\": string, \"auto_crop\": bool}}");
    println!("\nProfiller: hizli, standart, renkli (varsayılan), yuksek, siyah-beyaz");
    println!();

    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
