//! Ícones vetoriais/supersampled do aplicativo desktop Ponte e da extensão do navegador.
//! Gera imagens RGBA com anti-aliasing 4x4 em memória (para a janela `eframe`/`egui` e cabeçalho)
//! e exporta arquivos `.png`, `.ico` e `.svg` determinísticos para o executável Windows e a extensão MV3.

use anyhow::Result;
use eframe::egui;
use image::{ImageFormat, RgbaImage};
use std::io::Cursor;
use std::path::Path;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IconVariant {
    /// Ícone principal do aplicativo desktop Windows.
    App,
    /// Ícone da extensão do navegador (otimizado para 16x16..128x128 com selo de captura).
    Extension,
}

/// Retorna o `egui::IconData` (64x64 RGBA) para a janela nativa do aplicativo.
pub fn app_window_icon() -> egui::IconData {
    let size = 64u32;
    let rgba = render_rgba(size, IconVariant::App);
    egui::IconData {
        rgba,
        width: size,
        height: size,
    }
}

/// Retorna uma `egui::ColorImage` para exibição no cabeçalho interno do aplicativo.
pub fn app_header_color_image(size: usize) -> egui::ColorImage {
    let rgba = render_rgba(size as u32, IconVariant::App);
    egui::ColorImage::from_rgba_unmultiplied([size, size], &rgba)
}

/// Renderiza o ícone em um buffer RGBA (`size * size * 4` bytes) com supersampling 4x4.
pub fn render_rgba(size: u32, variant: IconVariant) -> Vec<u8> {
    let mut out = vec![0u8; (size * size * 4) as usize];
    let ss = 4u32;
    let inv_size = 1.0 / (size as f32);

    for py in 0..size {
        for px in 0..size {
            let mut r_acc = 0.0f32;
            let mut g_acc = 0.0f32;
            let mut b_acc = 0.0f32;
            let mut a_acc = 0.0f32;

            for sy in 0..ss {
                for sx in 0..ss {
                    let u = (px as f32 + (sx as f32 + 0.5) / (ss as f32)) * inv_size;
                    let v = (py as f32 + (sy as f32 + 0.5) / (ss as f32)) * inv_size;
                    let [r, g, b, a] = sample_icon(u, v, variant);
                    // Premultiplied accumulation for clean alpha edges
                    r_acc += r * a;
                    g_acc += g * a;
                    b_acc += b * a;
                    a_acc += a;
                }
            }

            let samples = (ss * ss) as f32;
            let a_final = (a_acc / samples).clamp(0.0, 1.0);
            let (r_final, g_final, b_final) = if a_final > 1e-5 {
                (
                    (r_acc / a_acc).clamp(0.0, 1.0),
                    (g_acc / a_acc).clamp(0.0, 1.0),
                    (b_acc / a_acc).clamp(0.0, 1.0),
                )
            } else {
                (0.0, 0.0, 0.0)
            };

            let idx = ((py * size + px) * 4) as usize;
            out[idx] = (r_final * 255.0 + 0.5) as u8;
            out[idx + 1] = (g_final * 255.0 + 0.5) as u8;
            out[idx + 2] = (b_final * 255.0 + 0.5) as u8;
            out[idx + 3] = (a_final * 255.0 + 0.5) as u8;
        }
    }
    out
}

fn sdf_rounded_box(u: f32, v: f32, cx: f32, cy: f32, hx: f32, hy: f32, r: f32) -> f32 {
    let dx = (u - cx).abs() - (hx - r);
    let dy = (v - cy).abs() - (hy - r);
    let ox = dx.max(0.0);
    let oy = dy.max(0.0);
    (ox * ox + oy * oy).sqrt() + dx.max(dy).min(0.0) - r
}

fn blend(dst: &mut [f32; 3], src: [f32; 3], alpha: f32) {
    let a = alpha.clamp(0.0, 1.0);
    dst[0] = dst[0] * (1.0 - a) + src[0] * a;
    dst[1] = dst[1] * (1.0 - a) + src[1] * a;
    dst[2] = dst[2] * (1.0 - a) + src[2] * a;
}

/// Avalia a cor `[R, G, B, A]` em coordenadas normalizadas `(u, v)` em `[0.0, 1.0]`.
fn sample_icon(u: f32, v: f32, variant: IconVariant) -> [f32; 4] {
    // 1. Squircle background mask
    let bg_dist = sdf_rounded_box(u, v, 0.5, 0.5, 0.465, 0.465, 0.21);
    if bg_dist > 0.0 {
        return [0.0, 0.0, 0.0, 0.0];
    }

    // Deep petrol teal gradient (#1D6474 -> #0D333C)
    let t = ((u * 0.45) + (v * 0.75)).clamp(0.0, 1.0);
    let mut rgb = [
        0.114 * (1.0 - t) + 0.051 * t,
        0.392 * (1.0 - t) + 0.200 * t,
        0.455 * (1.0 - t) + 0.235 * t,
    ];

    // Subtle radial glow behind the document and bridge
    let glow_d = ((u - 0.50).powi(2) + (v - 0.44).powi(2)).sqrt();
    if glow_d < 0.40 {
        let glow = (1.0 - glow_d / 0.40).powi(2) * 0.16;
        blend(&mut rgb, [0.24, 0.65, 0.74], glow);
    }

    // Inner gold/amber rim border (#E09F24)
    if bg_dist > -0.028 && bg_dist < -0.008 {
        blend(&mut rgb, [0.88, 0.63, 0.15], 0.42);
    }

    // 2. Official Document Sheet (upper-middle region, rising from behind the bridge)
    let doc_left = 0.29;
    let doc_right = 0.71;
    let doc_top = 0.14;
    let doc_bottom = 0.63;
    let fold_size = 0.115;

    let inside_doc_box = u >= doc_left && u <= doc_right && v >= doc_top && v <= doc_bottom;
    let cut_corner = (u - (doc_right - fold_size)) - (v - doc_top);
    if inside_doc_box && cut_corner <= 0.0 {
        // Warm parchment sheet (#FAF7F0 -> #EFE8D8)
        let doc_t = ((v - doc_top) / (doc_bottom - doc_top)).clamp(0.0, 1.0);
        let mut sheet = [
            0.980 - 0.045 * doc_t,
            0.969 - 0.055 * doc_t,
            0.941 - 0.085 * doc_t,
        ];

        // Folded dog-ear triangle in top-right
        if u >= doc_right - fold_size && v <= doc_top + fold_size {
            sheet = [0.84, 0.79, 0.69];
        }

        // Header accent bar on document (#D99500)
        if u >= 0.345 && u <= 0.53 && v >= 0.215 && v <= 0.252 {
            sheet = [0.851, 0.584, 0.000];
        }
        // Document text lines in petrol teal (#185461)
        if u >= 0.345 && u <= 0.645 && v >= 0.295 && v <= 0.326 {
            sheet = [0.094, 0.329, 0.380];
        }
        if u >= 0.345 && u <= 0.605 && v >= 0.362 && v <= 0.393 {
            sheet = [0.094, 0.329, 0.380];
        }
        if u >= 0.345 && u <= 0.565 && v >= 0.429 && v <= 0.458 {
            sheet = [0.220, 0.455, 0.510];
        }

        rgb = sheet;
    }

    // 3. Architectural Bridge ("Ponte") spanning across the lower half
    let gold_bright = [0.953, 0.710, 0.208]; // #F3B535
    let gold_deep = [0.835, 0.545, 0.047];   // #D58B0C
    let cream = [0.988, 0.976, 0.949];       // #FCF9F2

    // 3a. Suspension cables (parabolic arcs from towers x=0.25 and x=0.75)
    let cable_y = if u >= 0.11 && u < 0.25 {
        let s = (0.25 - u) / 0.14;
        0.41 + 0.21 * s * s
    } else if u >= 0.25 && u <= 0.75 {
        let s = (u - 0.50) / 0.25;
        0.575 - 0.165 * s * s
    } else if u > 0.75 && u <= 0.89 {
        let s = (u - 0.75) / 0.14;
        0.41 + 0.21 * s * s
    } else {
        -1.0
    };

    if cable_y > 0.0 {
        let cable_half_thick = match variant {
            IconVariant::App => 0.017,
            IconVariant::Extension => 0.022,
        };
        if (v - cable_y).abs() <= cable_half_thick {
            rgb = gold_bright;
        }
        // Vertical hangers between main cable and bridge deck (v = 0.62)
        for &hx in &[0.33f32, 0.41, 0.50, 0.59, 0.67] {
            if (u - hx).abs() <= 0.010 && v >= cable_y && v <= 0.62 {
                rgb = gold_deep;
            }
        }
    }

    // 3b. Lower Roman/structural arch under the deck (spanning x = 0.14..0.86)
    if u >= 0.13 && u <= 0.87 {
        let norm_x = (u - 0.50) / 0.36;
        let arch_center_y = 0.665 + 0.175 * norm_x * norm_x;
        let arch_half_thick = match variant {
            IconVariant::App => 0.028,
            IconVariant::Extension => 0.033,
        };
        if (v - arch_center_y).abs() <= arch_half_thick && v <= 0.85 {
            rgb = gold_bright;
        }
    }

    // 3c. Bridge Towers / Piers at x = 0.25 and x = 0.75
    let tower_half_w = match variant {
        IconVariant::App => 0.034,
        IconVariant::Extension => 0.038,
    };
    for &tx in &[0.25f32, 0.75f32] {
        if (u - tx).abs() <= tower_half_w && v >= 0.38 && v <= 0.84 {
            let edge = ((u - tx) / tower_half_w).abs();
            rgb = if edge > 0.65 { gold_deep } else { gold_bright };
        }
        // Tower cap
        if (u - tx).abs() <= tower_half_w * 1.25 && v >= 0.365 && v <= 0.395 {
            rgb = cream;
        }
    }

    // 3d. Horizontal Bridge Deck (x = 0.10..0.90, v = 0.605..0.658)
    let deck_dist = sdf_rounded_box(u, v, 0.50, 0.632, 0.40, 0.027, 0.013);
    if deck_dist <= 0.0 {
        rgb = if v < 0.625 { cream } else { gold_bright };
    }

    // 3e. Subtle water reflection wave at bottom (v = 0.87..0.895)
    if u >= 0.28 && u <= 0.72 && (v - 0.885).abs() <= 0.012 {
        blend(&mut rgb, [0.42, 0.82, 0.90], 0.65);
    }

    // 4. Extension variant badge: high-contrast capture arrow disk in bottom-right
    if variant == IconVariant::Extension {
        let bx = 0.77;
        let by = 0.77;
        let br = 0.185;
        let dist = ((u - bx).powi(2) + (v - by).powi(2)).sqrt();
        if dist <= br {
            if dist >= br - 0.030 {
                // Crisp ivory ring
                rgb = cream;
            } else {
                // Emerald-teal capture badge (#147A58)
                rgb = [0.078, 0.478, 0.345];
                // Downward capture arrow + tray inside badge
                let rx = (u - bx) / br;
                let ry = (v - by) / br;
                let in_shaft = rx.abs() <= 0.16 && (-0.52..=0.18).contains(&ry);
                let in_head = ry >= -0.05 && ry <= 0.38 && rx.abs() <= (0.38 - ry) * 1.15;
                let in_tray = ry >= 0.44 && ry <= 0.58 && rx.abs() <= 0.46;
                if in_shaft || in_head || in_tray {
                    rgb = cream;
                }
            }
        }
    }

    [rgb[0], rgb[1], rgb[2], 1.0]
}

/// Codifica uma imagem PNG em memória para o tamanho e variante solicitados.
pub fn encode_png(size: u32, variant: IconVariant) -> Result<Vec<u8>> {
    let raw = render_rgba(size, variant);
    let img = RgbaImage::from_raw(size, size, raw)
        .ok_or_else(|| anyhow::anyhow!("Falha ao construir buffer RGBA {size}x{size}"))?;
    let mut buf = Vec::new();
    img.write_to(&mut Cursor::new(&mut buf), ImageFormat::Png)?;
    Ok(buf)
}

/// Constrói um arquivo `.ico` multi-resolução (16, 32, 48 e 256 px) contendo quadros PNG.
pub fn encode_ico(variant: IconVariant) -> Result<Vec<u8>> {
    let sizes = [16u32, 32, 48, 256];
    let mut png_frames = Vec::with_capacity(sizes.len());
    for &sz in &sizes {
        png_frames.push((sz, encode_png(sz, variant)?));
    }

    let count = png_frames.len() as u16;
    let mut ico = Vec::new();
    // ICONDIR header (6 bytes)
    ico.extend_from_slice(&0u16.to_le_bytes()); // Reserved = 0
    ico.extend_from_slice(&1u16.to_le_bytes()); // Type = 1 (ICON)
    ico.extend_from_slice(&count.to_le_bytes()); // Count

    let mut offset = 6u32 + (count as u32) * 16;
    for (sz, data) in &png_frames {
        let dim_byte = if *sz >= 256 { 0u8 } else { *sz as u8 };
        ico.push(dim_byte); // bWidth
        ico.push(dim_byte); // bHeight
        ico.push(0); // bColorCount
        ico.push(0); // bReserved
        ico.extend_from_slice(&1u16.to_le_bytes()); // wPlanes = 1
        ico.extend_from_slice(&32u16.to_le_bytes()); // wBitCount = 32
        let len = data.len() as u32;
        ico.extend_from_slice(&len.to_le_bytes()); // dwBytesInRes
        ico.extend_from_slice(&offset.to_le_bytes()); // dwImageOffset
        offset += len;
    }

    for (_, data) in png_frames {
        ico.extend_from_slice(&data);
    }
    Ok(ico)
}

/// Retorna a representação vetorial SVG do ícone.
pub fn render_svg(variant: IconVariant) -> String {
    let badge = if variant == IconVariant::Extension {
        r##"
  <!-- Selo da extensão (captura SIGADOC -> Ponte) -->
  <circle cx="197" cy="197" r="46" fill="#147A58" stroke="#FCF9F2" stroke-width="7"/>
  <path d="M197 173 V203 M183 191 L197 206 L211 191 M177 216 H217" stroke="#FCF9F2" stroke-width="7" stroke-linecap="round" stroke-linejoin="round" fill="none"/>"##
    } else {
        ""
    };

    format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 256 256" width="256" height="256">
  <defs>
    <linearGradient id="bg" x1="0%" y1="0%" x2="100%" y2="100%">
      <stop offset="0%" stop-color="#1D6474"/>
      <stop offset="100%" stop-color="#0D333C"/>
    </linearGradient>
    <linearGradient id="gold" x1="0%" y1="0%" x2="0%" y2="100%">
      <stop offset="0%" stop-color="#F3B535"/>
      <stop offset="100%" stop-color="#D58B0C"/>
    </linearGradient>
  </defs>
  <!-- Base Squircle -->
  <rect x="9" y="9" width="238" height="238" rx="54" fill="url(#bg)" stroke="#E09F24" stroke-width="4" stroke-opacity="0.55"/>
  <!-- Documento Oficial (Fundo superior) -->
  <path d="M74 36 H153 L182 65 V161 H74 Z" fill="#FAF7F0"/>
  <path d="M153 36 V65 H182 Z" fill="#D6CBB5"/>
  <rect x="88" y="55" width="48" height="9" rx="3" fill="#D99500"/>
  <rect x="88" y="75" width="76" height="8" rx="3" fill="#185461"/>
  <rect x="88" y="92" width="66" height="8" rx="3" fill="#185461"/>
  <rect x="88" y="109" width="56" height="8" rx="3" fill="#387482"/>
  <!-- Cabos de suspensão e tirantes da Ponte -->
  <path d="M28 158 Q64 148 64 105 Q128 190 192 105 Q192 148 228 158" fill="none" stroke="url(#gold)" stroke-width="8" stroke-linecap="round"/>
  <line x1="84" y1="126" x2="84" y2="158" stroke="#D58B0C" stroke-width="5"/>
  <line x1="105" y1="141" x2="105" y2="158" stroke="#D58B0C" stroke-width="5"/>
  <line x1="128" y1="147" x2="128" y2="158" stroke="#D58B0C" stroke-width="5"/>
  <line x1="151" y1="141" x2="151" y2="158" stroke="#D58B0C" stroke-width="5"/>
  <line x1="172" y1="126" x2="172" y2="158" stroke="#D58B0C" stroke-width="5"/>
  <!-- Arco estrutural inferior -->
  <path d="M36 215 Q128 126 220 215" fill="none" stroke="url(#gold)" stroke-width="14" stroke-linecap="round"/>
  <!-- Pilares / Torres da Ponte -->
  <rect x="55" y="97" width="18" height="118" rx="4" fill="url(#gold)"/>
  <rect x="183" y="97" width="18" height="118" rx="4" fill="url(#gold)"/>
  <rect x="52" y="93" width="24" height="8" rx="3" fill="#FCF9F2"/>
  <rect x="180" y="93" width="24" height="8" rx="3" fill="#FCF9F2"/>
  <!-- Tabuleiro principal da Ponte -->
  <rect x="26" y="155" width="204" height="14" rx="7" fill="url(#gold)"/>
  <rect x="26" y="155" width="204" height="6" rx="3" fill="#FCF9F2"/>
  <!-- Reflexo sobre a água -->
  <rect x="72" y="224" width="112" height="6" rx="3" fill="#6BD1E6" fill-opacity="0.7"/>{badge}
</svg>
"##
    )
}

/// Grava os ativos de ícone do aplicativo (`assets/`) e da extensão (`extension/icons/`).
pub fn write_project_icon_assets(project_root: &Path) -> Result<()> {
    let assets_dir = project_root.join("assets");
    let ext_icons_dir = project_root.join("extension").join("icons");
    std::fs::create_dir_all(&assets_dir)?;
    std::fs::create_dir_all(&ext_icons_dir)?;

    std::fs::write(
        assets_dir.join("ponteproj.ico"),
        encode_ico(IconVariant::App)?,
    )?;
    std::fs::write(
        assets_dir.join("icon-256.png"),
        encode_png(256, IconVariant::App)?,
    )?;
    std::fs::write(
        assets_dir.join("ponteproj.svg"),
        render_svg(IconVariant::App),
    )?;

    for size in [16u32, 32, 48, 128] {
        std::fs::write(
            ext_icons_dir.join(format!("icon-{size}.png")),
            encode_png(size, IconVariant::Extension)?,
        )?;
    }
    std::fs::write(
        ext_icons_dir.join("ponteproj.svg"),
        render_svg(IconVariant::Extension),
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn generates_and_validates_app_and_extension_icons() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        write_project_icon_assets(&root).unwrap();

        let ico_bytes = std::fs::read(root.join("assets/ponteproj.ico")).unwrap();
        // Valid ICO header: 00 00 01 00 04 00 (4 images)
        assert_eq!(&ico_bytes[0..6], &[0, 0, 1, 0, 4, 0]);

        for size in [16u32, 32, 48, 128] {
            let path = root.join(format!("extension/icons/icon-{size}.png"));
            let bytes = std::fs::read(&path).unwrap();
            assert!(bytes.starts_with(b"\x89PNG\r\n\x1a\n"));
            let decoded = image::load_from_memory(&bytes).unwrap();
            assert_eq!(decoded.width(), size);
            assert_eq!(decoded.height(), size);
        }

        let win_icon = app_window_icon();
        assert_eq!(win_icon.width, 64);
        assert_eq!(win_icon.height, 64);
        assert_eq!(win_icon.rgba.len(), 64 * 64 * 4);
    }
}
