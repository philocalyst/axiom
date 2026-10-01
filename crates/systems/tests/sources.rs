use axiom_core::FileId;
use axiom_syntax::{Folder, parse};

#[test]
fn every_embedded_system_source_parses_as_v4() {
    for (index, (path, source)) in axiom_systems::SYSTEMS.iter().enumerate() {
        let file_id = FileId(u16::try_from(index).expect("few embedded system files"));
        let (_, diagnostics) = parse(file_id, source, Folder::of(path));
        assert!(diagnostics.is_empty(), "{path}: {diagnostics:?}");
    }
}
