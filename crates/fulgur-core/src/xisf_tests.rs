use super::*;
use crate::fits::{read_header, read_metadata, read_raw, read_rgb_roi, read_roi};
use crate::{Mosaic, RawFrame};
use sha2::Digest;
use std::fs;
use std::sync::atomic::AtomicBool;

const WIDTH: usize = 16;
const HEIGHT: usize = 12;

fn cancel() -> AtomicBool {
    AtomicBool::new(false)
}

/// Every sample differs, so a wrong layout or byte order cannot pass.
fn truth(channel: usize, x: usize, y: usize) -> u16 {
    (1000 + channel * 20000 + y * 311 + x * 17) as u16
}

/// Planar samples of `channels` planes as bytes of `sample`.
fn planar(channels: usize, encode: &dyn Fn(u16) -> Vec<u8>) -> Vec<u8> {
    let mut bytes = vec![];
    for channel in 0..channels {
        for y in 0..HEIGHT {
            for x in 0..WIDTH {
                bytes.extend(encode(truth(channel, x, y)));
            }
        }
    }
    bytes
}

fn little(value: u16) -> Vec<u8> {
    value.to_le_bytes().to_vec()
}

fn write_unit(path: &Path, image: &str, keywords: &str, blocks: &[u8], reserve: usize) {
    let mut xml = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<xisf version=\"1.0\" xmlns=\"http://www.pixinsight.com/xisf\">\n{image}{keywords}</Image>\n</xisf>"
    );
    xml = xml.replace("@START", &reserve.to_string());
    assert!(16 + xml.len() <= reserve);
    let mut bytes = b"XISF0100".to_vec();
    bytes.extend((xml.len() as u32).to_le_bytes());
    bytes.extend([0; 4]);
    bytes.extend(xml.as_bytes());
    bytes.resize(reserve, 0);
    bytes.extend(blocks);
    fs::write(path, bytes).unwrap();
}

const KEYWORDS: &str = "<FITSKeyword name=\"EXPTIME\" value=\"300.\" comment=\"s\"/>\n<FITSKeyword name=\"OBJECT\" value=\"'M 33'\" comment=\"\"/>\n<FITSKeyword name=\"BZERO\" value=\"32768\" comment=\"\"/>\n<FITSKeyword name=\"HISTORY\" value=\"\" comment=\"x\"/>\n";

/// A 16-bit mono image with `attributes` added to its element.
fn unit(path: &Path, attributes: &str, blocks: &[u8]) {
    let image = format!(
        "<Image geometry=\"{WIDTH}:{HEIGHT}:1\" sampleFormat=\"UInt16\" colorSpace=\"Gray\" {attributes}>\n"
    );
    write_unit(path, &image, KEYWORDS, blocks, 4096);
}

fn assert_mono(path: &Path) {
    let frame: RawFrame = read_raw(path).unwrap();
    assert_eq!(frame.header.info.mosaic, Mosaic::Mono);
    assert_eq!(frame.header.info.exposure_seconds, 300.);
    assert_eq!(frame.header.info.object.as_deref(), Some("M 33"));
    for y in 0..HEIGHT {
        for x in 0..WIDTH {
            assert_eq!(frame.pixels[y * WIDTH + x], f32::from(truth(0, x, y)));
        }
    }
    let roi = read_roi(path, &frame.header, [2, 4, 8, 6], &cancel()).unwrap();
    assert_eq!(roi.pixels[0], f32::from(truth(0, 2, 4)));
    assert_eq!(roi.pixels[8 * 6 - 1], f32::from(truth(0, 9, 9)));
    // A mirrored camera reads its rows reversed, in any rectangle.
    let mut mirrored = frame.header.clone();
    mirrored.reverse_row_order();
    let roi = read_roi(path, &mirrored, [2, 4, 8, 6], &cancel()).unwrap();
    assert_eq!(roi.pixels[0], f32::from(truth(0, 2, HEIGHT - 5)));
    assert_eq!(
        roi.pixels[8 * 6 - 1],
        f32::from(truth(0, 9, HEIGHT - 5 - 5))
    );
}

fn shuffled(input: &[u8], item: usize) -> Vec<u8> {
    let items = input.len() / item;
    let mut out = vec![];
    for byte in 0..item {
        out.extend((0..items).map(|i| input[i * item + byte]));
    }
    out.extend(&input[items * item..]);
    out
}

fn squeezed(codec: &str, data: &[u8]) -> Vec<u8> {
    match codec {
        "zlib" => miniz_oxide::deflate::compress_to_vec_zlib(data, 6),
        "lz4" | "lz4hc" => lz4_flex::block::compress(data),
        "zstd" => {
            ruzstd::encoding::compress_to_vec(data, ruzstd::encoding::CompressionLevel::Fastest)
        }
        _ => unreachable!(),
    }
}

#[test]
fn uncompressed_attachment_reads_like_fits_samples_in_both_byte_orders_and_layouts() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.xisf");
    let data = planar(1, &little);
    unit(
        &path,
        &format!("location=\"attachment:@START:{}\"", data.len()),
        &data,
    );
    assert_mono(&path);
    let big = planar(1, &|v| v.to_be_bytes().to_vec());
    unit(
        &path,
        &format!(
            "byteOrder=\"big\" location=\"attachment:@START:{}\"",
            big.len()
        ),
        &big,
    );
    assert_mono(&path);
}

#[test]
fn every_codec_with_and_without_shuffling_and_subblocks_gives_the_same_pixels() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.xisf");
    let data = planar(1, &little);
    for codec in ["zlib", "lz4", "lz4hc", "zstd"] {
        for shuffle in [false, true] {
            let prepared = if shuffle {
                shuffled(&data, 2)
            } else {
                data.clone()
            };
            let name = if shuffle {
                format!("{codec}+sh:{}:2", data.len())
            } else {
                format!("{codec}:{}", data.len())
            };
            let stored = squeezed(codec, &prepared);
            unit(
                &path,
                &format!(
                    "compression=\"{name}\" location=\"attachment:@START:{}\"",
                    stored.len()
                ),
                &stored,
            );
            assert_mono(&path);
            // Two subblocks: one compressed, one stored as is (revision 2).
            let cut = prepared.len() / 2;
            let first = squeezed(codec, &prepared[..cut]);
            let mut stored = first.clone();
            stored.extend(&prepared[cut..]);
            let rest = prepared.len() - cut;
            unit(
                &path,
                &format!(
                    "compression=\"{name}\" subblocks=\"{},{cut}:{rest},{rest}\" location=\"attachment:@START:{}\"",
                    first.len(),
                    stored.len()
                ),
                &stored,
            );
            assert_mono(&path);
        }
    }
}

#[test]
fn a_shuffle_remainder_stays_at_the_end() {
    let data: Vec<u8> = (0..11).collect();
    assert_eq!(unshuffle(&shuffled(&data, 4), 4), data);
    assert_eq!(unshuffle(&data, 1), data);
}

#[test]
fn checksums_are_verified_before_any_pixel_is_used() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.xisf");
    let data = planar(1, &little);
    let hex = |bytes: &[u8]| bytes.iter().map(|b| format!("{b:02x}")).collect::<String>();
    let digests = [
        ("sha-1", hex(&<sha1::Sha1 as sha1::Digest>::digest(&data))),
        ("sha1", hex(&<sha1::Sha1 as sha1::Digest>::digest(&data))),
        ("sha-256", hex(&sha2::Sha256::digest(&data))),
        ("sha512", hex(&sha2::Sha512::digest(&data))),
        (
            "sha3-256",
            hex(&<sha3::Sha3_256 as sha3::Digest>::digest(&data)),
        ),
        (
            "sha3-512",
            hex(&<sha3::Sha3_512 as sha3::Digest>::digest(&data)),
        ),
    ];
    for (name, digest) in digests {
        let attributes = format!(
            "checksum=\"{name}:{digest}\" location=\"attachment:@START:{}\"",
            data.len()
        );
        unit(&path, &attributes, &data);
        assert_mono(&path);
        let mut damaged = data.clone();
        damaged[5] ^= 1;
        unit(&path, &attributes, &damaged);
        assert!(read_raw(&path).is_err(), "{name}");
    }
}

#[test]
fn sample_formats_bounds_and_offset_map_onto_the_adu_scale() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.xisf");
    let value = |v: u16| f64::from(v) / 65535.;
    let cases: [(&str, &str, Vec<u8>); 5] = [
        (
            "Float32",
            "bounds=\"0:1\"",
            planar(1, &|v| (value(v) as f32).to_le_bytes().to_vec()),
        ),
        (
            "Float64",
            "bounds=\"0:1\"",
            planar(1, &|v| value(v).to_le_bytes().to_vec()),
        ),
        (
            "Float32",
            "bounds=\"-1:1\"",
            planar(1, &|v| ((value(v) * 2. - 1.) as f32).to_le_bytes().to_vec()),
        ),
        (
            "UInt32",
            "",
            planar(1, &|v| (u32::from(v) * 65537).to_le_bytes().to_vec()),
        ),
        (
            "Float32",
            "bounds=\"0:1\" offset=\"0.25\"",
            planar(1, &|v| ((value(v) + 0.25) as f32).to_le_bytes().to_vec()),
        ),
    ];
    for (format, extra, data) in cases {
        let image = format!(
            "<Image geometry=\"{WIDTH}:{HEIGHT}:1\" sampleFormat=\"{format}\" {extra} colorSpace=\"Gray\" location=\"attachment:@START:{}\">\n",
            data.len()
        );
        write_unit(&path, &image, KEYWORDS, &data, 4096);
        let frame = read_raw(&path).unwrap();
        assert_eq!(
            frame.header.info.bitpix,
            if format == "UInt32" {
                32
            } else if format == "Float64" {
                -64
            } else {
                -32
            }
        );
        for y in 0..HEIGHT {
            for x in 0..WIDTH {
                let actual = frame.pixels[y * WIDTH + x];
                let expected = f32::from(truth(0, x, y));
                assert!(
                    (actual - expected).abs() < 0.02,
                    "{format} {extra}: {actual} {expected}"
                );
            }
        }
    }
    let bytes: Vec<u8> = (0..WIDTH * HEIGHT).map(|i| (i % 256) as u8).collect();
    let image = format!(
        "<Image geometry=\"{WIDTH}:{HEIGHT}:1\" sampleFormat=\"UInt8\" location=\"attachment:@START:{}\">\n",
        bytes.len()
    );
    write_unit(&path, &image, KEYWORDS, &bytes, 4096);
    assert_eq!(read_raw(&path).unwrap().pixels[3], 3. * 257.);
}

#[test]
fn rgb_images_read_in_planar_and_normal_storage_and_never_as_a_cfa() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("rgb.xisf");
    let keywords =
        format!("{KEYWORDS}<FITSKeyword name=\"BAYERPAT\" value=\"'RGGB'\" comment=\"\"/>\n");
    let planes = planar(3, &little);
    let mut normal = vec![];
    for y in 0..HEIGHT {
        for x in 0..WIDTH {
            for channel in 0..3 {
                normal.extend(little(truth(channel, x, y)));
            }
        }
    }
    for (storage, data) in [("Planar", planes), ("Normal", normal)] {
        let image = format!(
            "<Image geometry=\"{WIDTH}:{HEIGHT}:3\" sampleFormat=\"UInt16\" colorSpace=\"RGB\" pixelStorage=\"{storage}\" location=\"attachment:@START:{}\">\n",
            data.len()
        );
        write_unit(&path, &image, &keywords, &data, 4096);
        let header = read_header(&path).unwrap();
        assert_eq!(header.info.mosaic, Mosaic::Rgb);
        assert_eq!(header.info.bayer_pattern, None);
        let rgb = read_rgb_roi(&header, [2, 4, 8, 6], &cancel(), vec![]).unwrap();
        let expected = [0, 1, 2].map(|c| f32::from(truth(c, 3, 5)));
        assert_eq!(rgb.pixels[8 + 1], expected, "{storage}");
        assert_eq!(
            read_raw(&path).unwrap().pixels[0],
            f32::from(truth(1, 0, 0))
        );
        let meta = read_metadata(&path).unwrap();
        assert_eq!((meta.planes, meta.bayer_pattern), (3, None));
    }
}

#[test]
fn a_color_filter_array_element_declares_the_mosaic() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.xisf");
    let data = planar(1, &little);
    let image = format!(
        "<Image geometry=\"{WIDTH}:{HEIGHT}:1\" sampleFormat=\"UInt16\" colorSpace=\"Gray\" location=\"attachment:@START:{}\">\n",
        data.len()
    );
    let keywords =
        format!("{KEYWORDS}<ColorFilterArray pattern=\"GBRG\" width=\"2\" height=\"2\"/>\n");
    write_unit(&path, &image, &keywords, &data, 4096);
    let info = read_header(&path).unwrap().info;
    assert_eq!(info.mosaic, Mosaic::Bayer);
    assert_eq!(info.bayer_pattern, Some(BayerPattern::GBRG));
    let keywords = format!(
        "{KEYWORDS}<ColorFilterArray pattern=\"GGRGGBGGBGGR\" width=\"3\" height=\"4\"/>\n"
    );
    write_unit(&path, &image, &keywords, &data, 4096);
    assert!(
        read_header(&path).is_err(),
        "X-Trans is refused, not read as mono"
    );
}

#[test]
fn properties_fill_keywords_the_unit_does_not_carry_and_old_style_units_still_read() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.xisf");
    let data = planar(1, &little);
    let image = format!(
        "<Image id=\"main\" geometry=\"{WIDTH}:{HEIGHT}:1\" sampleFormat=\"UInt16\" imageType=\"Light\" location=\"attachment:@START:{}\">\n",
        data.len()
    );
    let name = base64::engine::general_purpose::STANDARD.encode("Ha 7nm");
    let keywords = format!(
        "<Property id=\"Instrument:ExposureTime\" type=\"Float32\" value=\"120\"/>\n<Property id=\"Observation:Time:Start\" type=\"TimePoint\" value=\"2015-01-23T19:52:31.46Z\"/>\n<Property id=\"Instrument:Camera:Name\" type=\"String\">Canon EOS 7D</Property>\n<Property id=\"Instrument:Filter:Name\" type=\"String\" location=\"inline:base64\">{name}</Property>\n"
    );
    write_unit(&path, &image, &keywords, &data, 4096);
    let info = read_header(&path).unwrap().info;
    assert_eq!(info.exposure_seconds, 120.);
    assert_eq!(info.date_obs.as_deref(), Some("2015-01-23T19:52:31.46"));
    assert_eq!(info.filter.as_deref(), Some("Ha 7nm"));
    let meta = read_metadata(&path).unwrap();
    assert_eq!(meta.instrument.as_deref(), Some("Canon EOS 7D"));
    assert_eq!(meta.image_type.as_deref(), Some("Light"));
}

#[test]
fn embedded_blocks_and_detached_signatures_are_accepted() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.xisf");
    let data = planar(1, &little);
    for (encoding, text) in [
        (
            "base64",
            base64::engine::general_purpose::STANDARD.encode(&data),
        ),
        ("hex", data.iter().map(|b| format!("{b:02x}")).collect()),
    ] {
        let image = format!(
            "<Image geometry=\"{WIDTH}:{HEIGHT}:1\" sampleFormat=\"UInt16\" location=\"embedded\">\n<Data encoding=\"{encoding}\">\n{text}\n</Data>\n"
        );
        write_unit(&path, &image, KEYWORDS, &[], 8192);
        assert_mono(&path);
    }
    let attributes = format!("location=\"attachment:@START:{}\"", data.len());
    unit(&path, &attributes, &data);
    let mut bytes = fs::read(&path).unwrap();
    let length = u32::from_le_bytes(bytes[8..12].try_into().unwrap()) as usize;
    let signature = b"\n<Signature xmlns=\"http://www.w3.org/2000/09/xmldsig#\"/>";
    bytes[16 + length..16 + length + signature.len()].copy_from_slice(signature);
    bytes[8..12].copy_from_slice(&((length + signature.len()) as u32).to_le_bytes());
    fs::write(&path, bytes).unwrap();
    assert_mono(&path);
}

#[test]
fn the_first_light_image_of_a_unit_is_used_and_unsupported_images_are_skipped() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.xisf");
    let data = planar(1, &little);
    let image = format!(
        "<Image geometry=\"4:4:4:1\" sampleFormat=\"UInt16\" location=\"attachment:@START:128\"/>\n<Image geometry=\"{WIDTH}:{HEIGHT}:1\" sampleFormat=\"Complex32\" location=\"attachment:@START:4\"/>\n<Image geometry=\"{WIDTH}:{HEIGHT}:1\" sampleFormat=\"UInt16\" imageType=\"RejectionMapHigh\" location=\"attachment:@START:{0}\"/>\n<Image geometry=\"{WIDTH}:{HEIGHT}:1\" sampleFormat=\"UInt16\" imageType=\"Light\" location=\"attachment:@START:{0}\">\n",
        data.len()
    );
    write_unit(&path, &image, KEYWORDS, &data, 4096);
    assert_mono(&path);
}

fn xisb(path: &Path, blocks: &[(u64, &[u8])]) -> Vec<(u64, u64)> {
    let mut bytes = b"XISB0100".to_vec();
    bytes.extend([0; 8]);
    // Two index nodes, one block each, to walk the linked list.
    let mut position = 16;
    let mut places = vec![];
    for (i, (id, data)) in blocks.iter().enumerate() {
        let node = position;
        let start = node + 16 + 40;
        let next = if i + 1 < blocks.len() {
            start + data.len() as u64
        } else {
            0
        };
        bytes.extend(1_u32.to_le_bytes());
        bytes.extend([0; 4]);
        bytes.extend(next.to_le_bytes());
        for word in [*id, start, data.len() as u64, 0, 0] {
            bytes.extend(word.to_le_bytes());
        }
        bytes.extend(*data);
        places.push((start, data.len() as u64));
        position = start + data.len() as u64;
    }
    fs::write(path, bytes).unwrap();
    places
}

#[test]
fn distributed_units_read_blocks_from_data_block_files_and_plain_paths() {
    let dir = tempfile::tempdir().unwrap();
    let data = planar(1, &little);
    let decoy = vec![7; 64];
    xisb(
        &dir.path().join("blocks.xisb"),
        &[(0x1234, &decoy), (0x7a73_526b_584c_6167, &data)],
    );
    fs::write(dir.path().join("plain.bin"), &data).unwrap();
    let absolute = dir
        .path()
        .join("plain.bin")
        .to_string_lossy()
        .replace('\\', "/");
    for (index, location) in [
        "path(@header_dir/blocks.xisb):0x7a73526b584c6167".to_owned(),
        "path(@header_dir/plain.bin)".to_owned(),
        format!("path({absolute})"),
        format!("url(file:///{}):", absolute.trim_start_matches('/'))
            .trim_end_matches(':')
            .to_owned(),
    ]
    .into_iter()
    .enumerate()
    {
        let path = dir.path().join(format!("u{index}.xish"));
        let xml = format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<xisf version=\"1.0\" xmlns=\"http://www.pixinsight.com/xisf\">\n<Image geometry=\"{WIDTH}:{HEIGHT}:1\" sampleFormat=\"UInt16\" location=\"{location}\">\n{KEYWORDS}</Image>\n</xisf>"
        );
        fs::write(&path, xml).unwrap();
        assert_mono(&path);
    }
    let remote = dir.path().join("remote.xish");
    fs::write(
        &remote,
        format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<xisf version=\"1.0\" xmlns=\"http://www.pixinsight.com/xisf\">\n<Image geometry=\"{WIDTH}:{HEIGHT}:1\" sampleFormat=\"UInt16\" location=\"url(https://example.org/b.xisb):0x1\">\n{KEYWORDS}</Image>\n</xisf>"
        ),
    )
    .unwrap();
    assert!(read_header(&remote).is_err(), "no network access");
}

#[test]
fn damaged_units_are_refused_and_a_changed_file_is_noticed() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.xisf");
    let data = planar(1, &little);
    let attributes = format!("location=\"attachment:@START:{}\"", data.len());
    unit(&path, &attributes, &data);
    let header = read_header(&path).unwrap();
    let good = fs::read(&path).unwrap();
    fs::write(&path, &good[..good.len() - 1]).unwrap();
    assert!(read_roi(&path, &header, [0, 0, 8, 6], &cancel()).is_err());
    assert!(read_header(&path).is_err(), "truncated block");
    let mut bad = good.clone();
    bad[0] = b'Y';
    fs::write(&path, bad).unwrap();
    assert!(read_header(&path).is_err());
    let text =
        String::from_utf8_lossy(&good).replace("version=\"1.0\" xmlns", "version=\"2.0\" xmlns");
    fs::write(&path, text.as_bytes()).unwrap();
    assert!(read_header(&path).is_err());
    let wrong = format!("location=\"attachment:@START:{}\"", data.len() - 2);
    unit(&path, &wrong, &data);
    assert!(
        read_header(&path).is_err(),
        "block smaller than the geometry"
    );
    assert!(crate::fits::is_input(Path::new("x/a.XISF")));
    assert!(crate::fits::is_input(Path::new("x/a.xish")));
    assert!(!crate::fits::is_input(Path::new("x/._a.xisf")));
    assert!(!crate::fits::is_input(Path::new("x/a.xisb")));
}

#[test]
fn compressed_regions_share_a_decoded_band() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.xisf");
    let data = planar(1, &little);
    let stored = squeezed("zlib", &data);
    unit(
        &path,
        &format!(
            "compression=\"zlib:{}\" location=\"attachment:@START:{}\"",
            data.len(),
            stored.len()
        ),
        &stored,
    );
    let header = read_header(&path).unwrap();
    let image = header.xisf.clone().unwrap();
    assert_eq!(image.band_row_bytes(), WIDTH * 4);
    image.set_band_rows(8);
    let mut out = vec![0.; 6 * 4];
    image
        .read_region(0, [2, 2, 6, 4], &mut out, &cancel())
        .unwrap();
    assert_eq!(out[0], f32::from(truth(0, 2, 2)));
    let first = image.band.lock().unwrap().as_ref().unwrap().first;
    image
        .read_region(0, [8, 3, 6, 4], &mut out, &cancel())
        .unwrap();
    assert_eq!(out[0], f32::from(truth(0, 8, 3)));
    assert_eq!(image.band.lock().unwrap().as_ref().unwrap().first, first);
    image
        .read_region(0, [0, 8, 6, 4], &mut out, &cancel())
        .unwrap();
    assert_eq!(out[0], f32::from(truth(0, 0, 8)));
    image.set_band_rows(0);
    image
        .read_region(0, [0, 0, 6, 4], &mut out, &cancel())
        .unwrap();
    assert!(image.band.lock().unwrap().is_none(), "no memory, no band");
}

/// Optional real-data gate: every unit below the folder must open.
#[test]
fn real_units_open_when_a_folder_is_given() {
    let Ok(folder) = std::env::var("FULGURSTACK_TEST_XISF_DIR") else {
        return;
    };
    fn walk(dir: &Path, files: &mut Vec<PathBuf>) {
        for entry in fs::read_dir(dir).unwrap().flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(&path, files);
            } else if is_xisf(&path) && crate::fits::is_input(&path) {
                files.push(path);
            }
        }
    }
    let mut files = vec![];
    walk(Path::new(&folder), &mut files);
    let (mut mono, mut bayer, mut rgb) = (0, 0, 0);
    let mut failures = vec![];
    for path in &files {
        match read_header(path) {
            Ok(header) => match header.info.mosaic {
                Mosaic::Rgb => rgb += 1,
                Mosaic::Bayer => bayer += 1,
                _ => mono += 1,
            },
            Err(error) => failures.push(format!("{}: {error}", path.display())),
        }
    }
    println!(
        "XISF units {} · mono {mono} · Bayer {bayer} · RGB {rgb} · failed {}",
        files.len(),
        failures.len()
    );
    for failure in failures.iter().take(20) {
        println!("{failure}");
    }
    assert!(failures.is_empty());
}
