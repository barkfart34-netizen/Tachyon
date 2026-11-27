#![cfg(feature = "runtime-process")]

use horizon_mod_package::{Content, packages};
use std::collections::HashSet;
use std::fs;
use std::path::Path;

fn copy_dir_recursive(src: &Path, dst: &Path) -> std::io::Result<()> {
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let dst_path = dst.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir_recursive(&entry.path(), &dst_path)?;
        } else {
            fs::copy(entry.path(), &dst_path)?;
        }
    }
    Ok(())
}

#[test]
fn staged_create_decl_validates_cleanly() {
    let decl_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tachyon/content/_create_decl");
    assert!(
        decl_dir.is_dir(),
        "missing _create_decl dir at {}",
        decl_dir.display()
    );

    let tmp = tempfile::tempdir().expect("create tempdir");
    fs::copy(
        decl_dir.join("manifest.toml.template"),
        tmp.path().join("horizon-mod.toml"),
    )
    .expect("copy manifest.toml.template");
    for dir_name in ["assets", "data"] {
        copy_dir_recursive(&decl_dir.join(dir_name), &tmp.path().join(dir_name))
            .expect("copy _create_decl subdir");
    }

    let pkg = packages::read_bundled_native(tmp.path())
        .expect("staged _create_decl manifest must validate as bundled native");
    assert_eq!(pkg.manifest.id, "create");
    assert_eq!(pkg.manifest.version, "0.1.3");

    let content = Content::read_declaring(tmp.path(), &pkg.manifest, &[])
        .expect("_create_decl content must pass horizon-mod-package validation");

    assert_eq!(content.blocks.len(), 641);
    let block_ids: HashSet<&str> = content.blocks.iter().map(|b| b.id.as_str()).collect();
    for expected in [
        "create:adjustable_chain_gearshift",
        "create:chocolate",
        "create:honey",
        "create:copper_backtank",
        "create:netherite_backtank",
    ] {
        assert!(block_ids.contains(expected), "missing block {expected}");
    }

    let item_ids: HashSet<&str> = content
        .items
        .iter()
        .map(|i| i.id.as_str())
        .chain(
            content
                .blocks
                .iter()
                .filter(|b| b.item)
                .map(|b| b.id.as_str()),
        )
        .collect();
    assert_eq!(item_ids.len(), 688);
    for expected in [
        "create:chocolate_bucket",
        "create:honey_bucket",
        "create:copper_backtank",
        "create:netherite_backtank",
        "create:empty_blaze_burner",
        "create:vertical_gearbox",
    ] {
        assert!(item_ids.contains(expected), "missing item {expected}");
    }

    let fluid_ids: HashSet<&str> = content.fluids.iter().map(|f| f.id.as_str()).collect();
    assert!(fluid_ids.contains("create:chocolate"));
    assert!(fluid_ids.contains("create:honey"));
}
