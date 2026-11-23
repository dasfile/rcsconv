//! Parity with the Python reference on `golden/` data + synthetic checks.
//!
//! The data is not in git (`docs/CORPUS.md`), so when `golden/` is missing the
//! tests print "skipped" and pass — like `SkipTest` in Python.

use std::path::{Path, PathBuf};

use rcs_format::point::{decode_body, encode_body, BODY_SIZE};
use rcs_format::{metadata, point, properties, tile, tlv, writer, Container, Error, Metadata};

fn golden_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../golden")
}

/// Read a reference file; `None` when the data is missing (the test is skipped).
fn golden(name: &str) -> Option<Vec<u8>> {
    let path = golden_dir().join(name);
    match std::fs::read(&path) {
        Ok(data) => Some(data),
        Err(err) => {
            eprintln!("skipped: {} ({err})", path.display());
            None
        }
    }
}

/// Parse a reference file. Bytes are "leaked" with `Box::leak`: the container
/// borrows a slice and the test process lives for seconds, so this is cheaper
/// than copying.
fn parse(name: &str) -> Option<Container<'static>> {
    let data: &'static [u8] = Box::leak(golden(name)?.into_boxed_slice());
    Some(Container::parse(data).expect("container parse"))
}

/// `read -> write` must produce a byte-identical file.
fn assert_round_trip(name: &str) {
    let Some(original) = golden(name) else {
        return;
    };
    let data: &'static [u8] = Box::leak(original.clone().into_boxed_slice());
    let container = Container::parse(data).expect("container parse");
    let rebuilt = writer::serialize(&container, None, None).expect("serialise");
    assert_eq!(rebuilt.len(), original.len(), "{name}: file size");
    let ranges = writer::diff_ranges(&original, &rebuilt).expect("compare");
    assert!(ranges.is_empty(), "{name}: differences at {ranges:?}");
}

#[test]
fn round_trip_probe6() {
    assert_round_trip("probe6.rcs");
}

#[test]
fn round_trip_test1() {
    assert_round_trip("test1.rcs");
}

#[test]
fn round_trip_test2() {
    assert_round_trip("test2.rcs");
}

#[test]
fn round_trip_level_7() {
    assert_round_trip("level_7.rcs");
}

#[test]
fn round_trip_reference_files() {
    let dir = golden_dir().join("reference");
    let Ok(entries) = std::fs::read_dir(&dir) else {
        eprintln!("skipped: no {}", dir.display());
        return;
    };
    let mut checked = 0;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("rcs") {
            continue;
        }
        let Ok(bytes) = std::fs::read(&path) else { continue };
        let data: &'static [u8] = Box::leak(bytes.clone().into_boxed_slice());
        let container = Container::parse(data).expect("reference parse");
        let rebuilt = writer::serialize(&container, None, None).expect("serialise");
        let ranges = writer::diff_ranges(&bytes, &rebuilt).expect("compare");
        assert!(ranges.is_empty(), "{}: differences at {ranges:?}", path.display());
        checked += 1;
    }
    eprintln!("reference files checked: {checked}");
}

#[test]
fn header_layout_probe6() {
    let Some(container) = parse("probe6.rcs") else {
        return;
    };
    let header = container.header();
    assert_eq!(container.directory_offset(), 0x135, "directory at 0x135");
    assert_eq!(header.guid_offset, 0xE3, "GUID at 0xE3");
    assert_eq!(header.guid.len(), 38, "GUID string length");
    assert_eq!(header.word4, 84);
    assert_eq!(header.word8, 3);
    assert_eq!(header.flags, [1, 1, 0]);
    assert_eq!(container.block_types(), vec![3, 1, 11, 0], "export");
    assert_eq!(container.size(), 100_327);
    assert_eq!(container.tile_record_size(), Some(tile::TILE_RECORD_SIZE));
}


#[test]
fn tiles_and_points_probe6() {
    let Some(container) = parse("probe6.rcs") else {
        return;
    };
    let tiles = tile::iter_tiles(&container).expect("tiles");
    assert!(!tiles.is_empty(), "tiles found");
    let sum: u64 = tiles.iter().map(|tile| tile.count as u64).sum();
    assert_eq!(sum, container.header().count, "sum of tile counts");
    let points = point::decode_all(&container).expect("points");
    assert_eq!(points.len() as u64, container.header().count);
    let center = container.header().center;
    assert_eq!(points.len() as u64, container.header().count);

    // Coordinate reference is probe6.pts (colours are RGB in the file, BGR in RCS).
    let pts = std::fs::read_to_string(golden_dir().join("probe6.pts")).expect("probe6.pts");
    let expected: Vec<[f64; 3]> = pts
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            let mut parts = line.split_whitespace();
            let mut xyz = [0.0; 3];
            for slot in xyz.iter_mut() {
                *slot = parts.next().expect("coordinate").parse().expect("number");
            }
            xyz
        })
        .collect();
    assert_eq!(points.len(), expected.len(), "point count in probe6.pts");
    // Container point order is octree traversal, not `.pts` line order, so we
    // compare by presence: for every line look for a point within the
    // quantisation step (1 mm on x).
    for (index, want) in expected.iter().enumerate() {
        let found = points.iter().any(|point| {
            let world = point.world(center);
            (0..3).all(|axis| (world[axis] - want[axis]).abs() <= 1e-3)
        });
        assert!(found, "point {index} of probe6.pts not found in .rcs: {want:?}");
    }
}

#[test]
fn tile_block_rebuilds_block_bytes() {
    let Some(container) = parse("level_7.rcs") else {
        return;
    };
    let block = container.block_of_type(3).expect("type 3 block");
    let tiles = tile::iter_tiles(&container).expect("tiles");
    let rebuilt = tile::tile_block_bytes(&tiles).expect("block build");
    assert_eq!(rebuilt, container.read_block(block), "type 3 block byte-identical");
}

#[test]
fn single_point_bodies_round_trip() {
    let Some(container) = parse("probe6.rcs") else {
        return;
    };
    let mut checked = 0;
    for tile in tile::iter_tiles(&container).expect("tiles") {
        if tile.count != 1 {
            continue;
        }
        let payload = tile::tile_payload(&container, &tile).expect("payload");
        let body = &payload[payload.len() - BODY_SIZE..];
        let decoded = decode_body(body, tile.cube_min).expect("decode");
        let encoded = encode_body(&decoded, tile.cube_min).expect("encode");
        assert_eq!(encoded.as_slice(), body, "point body {} byte-identical", tile.index);
        checked += 1;
    }
    assert!(checked > 0, "single-point tiles found");
}

#[test]
fn metadata_words_match_octree() {
    for (name, want) in [("probe6.rcs", vec![1, 3]), ("level_7.rcs", vec![1, 8, 13, 43])] {
        let Some(container) = parse(name) else {
            return;
        };
        let block = container.block_of_type(1).expect("type 1 block");
        let meta = Metadata::parse(container.read_block(block)).expect("type 1 parse");
        assert_eq!(meta.levels(), want, "{name}: octree levels");
        assert!(meta.is_consistent(), "{name}: K = sum(words)");
        assert_eq!(meta.trailing, 0, "{name}: no extra bytes");
    }
}

#[test]
fn properties_block_of_export() {
    let Some(container) = parse("probe6.rcs") else {
        return;
    };
    let block = container.tlv_block().expect("properties block");
    let props = properties::Properties::parse(container.read_block(block));
    assert_eq!(props.version, properties::PROPS_VERSION);
    assert_eq!(props.u8_field(100), Some(1), "attribute schema version");
    assert_eq!(props.f32_field(101), Some(properties::PROPS_UPPER));
    assert_eq!(props.f32_field(102), Some(properties::PROPS_LOWER));
    assert_eq!(props.f64_field(203), Some(std::f64::consts::PI));
    assert_eq!(props.entries.len(), 10, "number of TLV fields");
}

#[test]
fn properties_block_rebuilds_134_bytes() {
    let built = properties::build_default_properties_block();
    assert_eq!(built.len(), 134);
    let Some(container) = parse("probe6.rcs") else {
        return;
    };
    let block = container.tlv_block().expect("properties block");
    assert_eq!(built, container.read_block(block), "properties block byte-identical");
}

#[test]
fn metadata_build_follows_invariant() {
    let block = metadata::build_metadata_block(&[1, 3], None).expect("build");
    assert_eq!(
        block.len(),
        metadata::METADATA_HEAD_SIZE + 4 * metadata::METADATA_RECORD_SIZE
    );
    let parsed = Metadata::parse(&block).expect("parse");
    assert_eq!(parsed.levels(), vec![1, 3]);
    assert_eq!(parsed.record_count, 4);
    assert!(parsed.is_consistent());
    assert!(metadata::build_metadata_block(&[1, 3], Some(5)).is_err());
    assert!(metadata::build_metadata_block(&[1; 40], None).is_err());
}

#[test]
fn metadata_block_rebuilds_golden_block() {
    // A `type 1` block built from the words must match the ReCap reference:
    // records are identical in every reference file, words are occupied tiles per level.
    for (name, words) in [
        ("probe6.rcs", vec![1u32, 3]),
        ("level_7.rcs", vec![1, 8, 13, 43]),
    ] {
        let Some(container) = parse(name) else {
            return;
        };
        let block = container.block_of_type(1).expect("type 1 block");
        let rebuilt = metadata::build_metadata_block(&words, None).expect("build");
        assert_eq!(rebuilt, container.read_block(block), "{name}: type 1 byte-identical");
    }
}

#[test]
fn bad_signature_is_rejected() {
    let mut data = vec![0u8; 0x200];
    data[..8].copy_from_slice(b"NOTADOCT");
    assert!(matches!(Container::parse(&data), Err(Error::BadSignature(_))));
    assert!(matches!(
        Container::parse(&data[..0x100]),
        Err(Error::TooShort { .. })
    ));
}

#[test]
fn tlv_stream_stops_at_garbage() {
    let mut blob = properties::tlv_entry(100, &[1]);
    blob.extend_from_slice(&[0xFF; 4]);
    let entries = tlv::parse_tlv(&blob, 0, 0);
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].id, 100);
    assert_eq!(entries[0].payload, vec![1]);
}
