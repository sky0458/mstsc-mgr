#[cfg(windows)]
use image::{RgbaImage, imageops::FilterType};
#[cfg(windows)]
use std::{env, io, path::Path};

#[cfg(windows)]
const PNG_SIGNATURE: &[u8; 8] = b"\x89PNG\r\n\x1a\n";

#[cfg(windows)]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("cargo:rerun-if-changed=assets/mstsc-mgr.ico");

    let source_ico = std::fs::read("assets/mstsc-mgr.ico")?;
    let source_png = largest_embedded_png(&source_ico)?;
    let source =
        image::load_from_memory_with_format(source_png, image::ImageFormat::Png)?.into_rgba8();

    let out_dir = env::var_os("OUT_DIR")
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "OUT_DIR is not set"))?;
    let resource_icon = std::path::PathBuf::from(out_dir).join("mstsc-mgr-resource.ico");
    write_classic_ico(&source, &resource_icon)?;
    let icon_path = resource_icon.to_str().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "generated icon path is not valid UTF-8",
        )
    })?;

    let mut resource = winresource::WindowsResource::new();
    resource
        .set_icon(icon_path)
        .set("InternalName", "mstsc-mgr.exe")
        .set("OriginalFilename", "mstsc-mgr.exe")
        .set("ProductName", "mstsc-mgr");
    resource.compile()?;
    Ok(())
}

#[cfg(windows)]
fn largest_embedded_png(ico: &[u8]) -> io::Result<&[u8]> {
    if read_u16(ico, 0)? != 0 || read_u16(ico, 2)? != 1 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "source icon has an invalid ICO header",
        ));
    }

    let count = usize::from(read_u16(ico, 4)?);
    let mut best: Option<(u32, &[u8])> = None;
    for index in 0..count {
        let entry_offset = 6_usize
            .checked_add(
                index
                    .checked_mul(16)
                    .ok_or_else(|| io::Error::other("ICO directory offset overflow"))?,
            )
            .ok_or_else(|| io::Error::other("ICO directory offset overflow"))?;
        let width_byte = *ico.get(entry_offset).ok_or_else(|| {
            io::Error::new(io::ErrorKind::UnexpectedEof, "truncated ICO directory")
        })?;
        let height_byte = *ico.get(entry_offset + 1).ok_or_else(|| {
            io::Error::new(io::ErrorKind::UnexpectedEof, "truncated ICO directory")
        })?;
        let width = if width_byte == 0 {
            256
        } else {
            u32::from(width_byte)
        };
        let height = if height_byte == 0 {
            256
        } else {
            u32::from(height_byte)
        };
        let length = usize::try_from(read_u32(ico, entry_offset + 8)?).map_err(io::Error::other)?;
        let data_offset =
            usize::try_from(read_u32(ico, entry_offset + 12)?).map_err(io::Error::other)?;
        let data_end = data_offset
            .checked_add(length)
            .ok_or_else(|| io::Error::other("ICO image offset overflow"))?;
        let payload = ico.get(data_offset..data_end).ok_or_else(|| {
            io::Error::new(io::ErrorKind::UnexpectedEof, "truncated ICO image data")
        })?;
        if !payload.starts_with(PNG_SIGNATURE) {
            continue;
        }

        let score = width.saturating_mul(height);
        let should_replace = match best {
            Some((best_score, _)) => score > best_score,
            None => true,
        };
        if should_replace {
            best = Some((score, payload));
        }
    }

    best.map(|(_, payload)| payload).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "source icon contains no embedded PNG image",
        )
    })
}

#[cfg(windows)]
fn read_u16(data: &[u8], offset: usize) -> io::Result<u16> {
    let end = offset
        .checked_add(2)
        .ok_or_else(|| io::Error::other("binary offset overflow"))?;
    let bytes = data
        .get(offset..end)
        .ok_or_else(|| io::Error::new(io::ErrorKind::UnexpectedEof, "truncated binary data"))?;
    Ok(u16::from_le_bytes([bytes[0], bytes[1]]))
}

#[cfg(windows)]
fn read_u32(data: &[u8], offset: usize) -> io::Result<u32> {
    let end = offset
        .checked_add(4)
        .ok_or_else(|| io::Error::other("binary offset overflow"))?;
    let bytes = data
        .get(offset..end)
        .ok_or_else(|| io::Error::new(io::ErrorKind::UnexpectedEof, "truncated binary data"))?;
    Ok(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

#[cfg(windows)]
fn write_classic_ico(source: &RgbaImage, path: &Path) -> io::Result<()> {
    let mut images = Vec::new();
    for size in [16_u8, 32, 48, 64] {
        let dimension = u32::from(size);
        let resized = image::imageops::resize(source, dimension, dimension, FilterType::Lanczos3);
        images.push((size, encode_dib(&resized)?));
    }

    let count = u16::try_from(images.len()).map_err(io::Error::other)?;
    let mut icon = Vec::new();
    push_u16(&mut icon, 0);
    push_u16(&mut icon, 1);
    push_u16(&mut icon, count);

    let mut offset = 6_u32 + u32::from(count) * 16;
    for (size, data) in &images {
        icon.extend_from_slice(&[*size, *size, 0, 0]);
        push_u16(&mut icon, 1);
        push_u16(&mut icon, 32);
        let length = u32::try_from(data.len()).map_err(io::Error::other)?;
        push_u32(&mut icon, length);
        push_u32(&mut icon, offset);
        offset = offset
            .checked_add(length)
            .ok_or_else(|| io::Error::other("ICO resource is too large"))?;
    }
    for (_, data) in images {
        icon.extend_from_slice(&data);
    }

    std::fs::write(path, icon)
}

#[cfg(windows)]
fn encode_dib(image: &RgbaImage) -> io::Result<Vec<u8>> {
    let (width, height) = image.dimensions();
    let doubled_height = height
        .checked_mul(2)
        .ok_or_else(|| io::Error::other("icon height overflow"))?;
    let image_bytes = width
        .checked_mul(height)
        .and_then(|value| value.checked_mul(4))
        .ok_or_else(|| io::Error::other("icon image size overflow"))?;

    let mut dib = Vec::new();
    push_u32(&mut dib, 40);
    push_i32(&mut dib, i32::try_from(width).map_err(io::Error::other)?);
    push_i32(
        &mut dib,
        i32::try_from(doubled_height).map_err(io::Error::other)?,
    );
    push_u16(&mut dib, 1);
    push_u16(&mut dib, 32);
    push_u32(&mut dib, 0);
    push_u32(&mut dib, image_bytes);
    push_i32(&mut dib, 0);
    push_i32(&mut dib, 0);
    push_u32(&mut dib, 0);
    push_u32(&mut dib, 0);

    for y in (0..height).rev() {
        for x in 0..width {
            let [red, green, blue, alpha] = image.get_pixel(x, y).0;
            dib.extend_from_slice(&[blue, green, red, alpha]);
        }
    }

    let mask_stride = width.div_ceil(32) * 4;
    let stride = usize::try_from(mask_stride).map_err(io::Error::other)?;
    for y in (0..height).rev() {
        let row_start = dib.len();
        dib.resize(row_start + stride, 0);
        for x in 0..width {
            if image.get_pixel(x, y).0[3] == 0 {
                let byte_index = usize::try_from(x / 8).map_err(io::Error::other)?;
                let bit_index = 7 - (x % 8);
                dib[row_start + byte_index] |= 1_u8 << bit_index;
            }
        }
    }

    Ok(dib)
}

#[cfg(windows)]
fn push_u16(buffer: &mut Vec<u8>, value: u16) {
    buffer.extend_from_slice(&value.to_le_bytes());
}

#[cfg(windows)]
fn push_u32(buffer: &mut Vec<u8>, value: u32) {
    buffer.extend_from_slice(&value.to_le_bytes());
}

#[cfg(windows)]
fn push_i32(buffer: &mut Vec<u8>, value: i32) {
    buffer.extend_from_slice(&value.to_le_bytes());
}

#[cfg(not(windows))]
fn main() {}
