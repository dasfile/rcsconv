//! Паритет с Python-эталоном на данных `golden/` + синтетические проверки.
//!
//! Данные в git не входят (`docs/CORPUS.md`), поэтому при отсутствии `golden/`
//! тесты печатают «пропуск» и завершаются успешно — как `SkipTest` в Python.

use std::path::{Path, PathBuf};

use rcs_format::point::{decode_body, encode_body, BODY_SIZE};
use rcs_format::{metadata, point, properties, tile, tlv, writer, Container, Error, Metadata};

fn golden_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../golden")
}

/// Прочитать эталон; `None`, если данных нет (тест тогда пропускается).
fn golden(name: &str) -> Option<Vec<u8>> {
    let path = golden_dir().join(name);
    match std::fs::read(&path) {
        Ok(data) => Some(data),
        Err(err) => {
            eprintln!("пропуск: {} ({err})", path.display());
            None
        }
    }
}

/// Разобрать эталон. Байты «утекают» в `Box::leak`: контейнер держит срез,
/// а процесс теста живёт секунды, поэтому это дешевле копирования.
fn parse(name: &str) -> Option<Container<'static>> {
    let data: &'static [u8] = Box::leak(golden(name)?.into_boxed_slice());
    Some(Container::parse(data).expect("разбор контейнера"))
}

/// `read → write` должен дать побайтово идентичный файл.
fn assert_round_trip(name: &str) {
    let Some(original) = golden(name) else {
        return;
    };
    let data: &'static [u8] = Box::leak(original.clone().into_boxed_slice());
    let container = Container::parse(data).expect("разбор контейнера");
    let rebuilt = writer::serialize(&container, None, None).expect("сериализация");
    assert_eq!(rebuilt.len(), original.len(), "{name}: размер файла");
    let ranges = writer::diff_ranges(&original, &rebuilt).expect("сравнение");
    assert!(ranges.is_empty(), "{name}: различия в {ranges:?}");
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
        eprintln!("пропуск: нет {}", dir.display());
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
        let container = Container::parse(data).expect("разбор reference");
        let rebuilt = writer::serialize(&container, None, None).expect("сериализация");
        let ranges = writer::diff_ranges(&bytes, &rebuilt).expect("сравнение");
        assert!(ranges.is_empty(), "{}: различия в {ranges:?}", path.display());
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
    assert_eq!(container.directory_offset(), 0x135, "директория на 0x135");
    assert_eq!(header.guid_offset, 0xE3, "GUID на 0xE3");
    assert_eq!(header.guid.len(), 38, "длина строки GUID");
    assert_eq!(header.word4, 84);
    assert_eq!(header.word8, 3);
    assert_eq!(header.flags, [1, 1, 0]);
    assert_eq!(container.block_types(), vec![3, 1, 11, 0], "экспорт");
    assert_eq!(container.size(), 100_327);
    assert_eq!(container.tile_record_size(), Some(tile::TILE_RECORD_SIZE));
}


#[test]
fn tiles_and_points_probe6() {
    let Some(container) = parse("probe6.rcs") else {
        return;
    };
    let tiles = tile::iter_tiles(&container).expect("узлы");
    assert!(!tiles.is_empty(), "узлы найдены");
    let sum: u64 = tiles.iter().map(|tile| tile.count as u64).sum();
    assert_eq!(sum, container.header().count, "сумма count по узлам");
    let points = point::decode_all(&container).expect("точки");
    assert_eq!(points.len() as u64, container.header().count);
    let center = container.header().center;
    assert_eq!(points.len() as u64, container.header().count);

    // Эталон координат — probe6.pts (цвета в файле RGB, в RCS порядок BGR).
    let pts = std::fs::read_to_string(golden_dir().join("probe6.pts")).expect("probe6.pts");
    let expected: Vec<[f64; 3]> = pts
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            let mut parts = line.split_whitespace();
            let mut xyz = [0.0; 3];
            for slot in xyz.iter_mut() {
                *slot = parts.next().expect("координата").parse().expect("число");
            }
            xyz
        })
        .collect();
    assert_eq!(points.len(), expected.len(), "число точек в probe6.pts");
    // Порядок точек в контейнере — обход октодерева, а не порядок строк `.pts`,
    // поэтому сверяем по совпадению: для каждой строки ищем точку в пределах
    // квантования (1 мм по x).
    for (index, want) in expected.iter().enumerate() {
        let found = points.iter().any(|point| {
            let world = point.world(center);
            (0..3).all(|axis| (world[axis] - want[axis]).abs() <= 1e-3)
        });
        assert!(found, "точка {index} из probe6.pts не найдена в .rcs: {want:?}");
    }
}

#[test]
fn tile_block_rebuilds_block_bytes() {
    let Some(container) = parse("level_7.rcs") else {
        return;
    };
    let block = container.block_of_type(3).expect("блок type 3");
    let tiles = tile::iter_tiles(&container).expect("узлы");
    let rebuilt = tile::tile_block_bytes(&tiles).expect("сборка блока");
    assert_eq!(rebuilt, container.read_block(block), "блок type 3 побайтово");
}

#[test]
fn single_point_bodies_round_trip() {
    let Some(container) = parse("probe6.rcs") else {
        return;
    };
    let mut checked = 0;
    for tile in tile::iter_tiles(&container).expect("узлы") {
        if tile.count != 1 {
            continue;
        }
        let payload = tile::tile_payload(&container, &tile).expect("payload");
        let body = &payload[payload.len() - BODY_SIZE..];
        let decoded = decode_body(body, tile.cube_min).expect("декодирование");
        let encoded = encode_body(&decoded, tile.cube_min).expect("кодирование");
        assert_eq!(encoded.as_slice(), body, "тело точки {} побайтово", tile.index);
        checked += 1;
    }
    assert!(checked > 0, "одноточечные узлы найдены");
}

#[test]
fn metadata_words_match_octree() {
    for (name, want) in [("probe6.rcs", vec![1, 3]), ("level_7.rcs", vec![1, 8, 13, 43])] {
        let Some(container) = parse(name) else {
            return;
        };
        let block = container.block_of_type(1).expect("блок type 1");
        let meta = Metadata::parse(container.read_block(block)).expect("разбор type 1");
        assert_eq!(meta.levels(), want, "{name}: уровни октодерева");
        assert!(meta.is_consistent(), "{name}: K = sum(words)");
        assert_eq!(meta.trailing, 0, "{name}: лишних байт нет");
    }
}

#[test]
fn properties_block_of_export() {
    let Some(container) = parse("probe6.rcs") else {
        return;
    };
    let block = container.tlv_block().expect("блок свойств");
    let props = properties::Properties::parse(container.read_block(block));
    assert_eq!(props.version, properties::PROPS_VERSION);
    assert_eq!(props.u8_field(100), Some(1), "версия схемы атрибутов");
    assert_eq!(props.f32_field(101), Some(properties::PROPS_UPPER));
    assert_eq!(props.f32_field(102), Some(properties::PROPS_LOWER));
    assert_eq!(props.f64_field(203), Some(std::f64::consts::PI));
    assert_eq!(props.entries.len(), 10, "число TLV-полей");
}

#[test]
fn properties_block_rebuilds_134_bytes() {
    let built = properties::build_default_properties_block();
    assert_eq!(built.len(), 134);
    let Some(container) = parse("probe6.rcs") else {
        return;
    };
    let block = container.tlv_block().expect("блок свойств");
    assert_eq!(built, container.read_block(block), "блок свойств побайтово");
}

#[test]
fn metadata_build_follows_invariant() {
    let block = metadata::build_metadata_block(&[1, 3], None).expect("сборка");
    assert_eq!(
        block.len(),
        metadata::METADATA_HEAD_SIZE + 4 * metadata::METADATA_RECORD_SIZE
    );
    let parsed = Metadata::parse(&block).expect("разбор");
    assert_eq!(parsed.levels(), vec![1, 3]);
    assert_eq!(parsed.record_count, 4);
    assert!(parsed.is_consistent());
    assert!(metadata::build_metadata_block(&[1, 3], Some(5)).is_err());
    assert!(metadata::build_metadata_block(&[1; 40], None).is_err());
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
