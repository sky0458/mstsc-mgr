#[cfg(windows)]
use image::{RgbaImage, imageops::FilterType};
#[cfg(windows)]
use std::{env, io, path::Path};

#[cfg(windows)]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("cargo:rerun-if-changed=assets/mstsc-mgr.ico");

    let source = image::open("assets/mstsc-mgr.ico")?.into_rgba8();
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
    push_i32(
        &mut dib,
        i32::try_from(width).map_err(io::Error::other)?,
    );
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
