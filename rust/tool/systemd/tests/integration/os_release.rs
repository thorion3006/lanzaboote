use std::fs;

use anyhow::{Context, Result};
use expect_test::expect;
use tempfile::tempdir;

use crate::common;

#[test]
fn generate_expected_os_release() -> Result<()> {
    let esp_mountpoint = tempdir()?;
    let tmpdir = tempdir()?;
    let profiles = tempdir()?;
    let toplevel = common::setup_toplevel(tmpdir.path())?;

    let generation_link =
        common::setup_generation_link_from_toplevel(&toplevel, profiles.path(), 1)
            .expect("Failed to setup generation link");

    let output0 = common::lanzaboote_install(0, esp_mountpoint.path(), vec![generation_link])?;
    assert!(output0.status.success());

    let stub_data = fs::read(common::image_path(&esp_mountpoint, 1, &toplevel)?)?;
    let os_release_section = pe_section(&stub_data, ".osrel")
        .context("Failed to read .osrelease PE section.")?
        .to_owned();

    let expected = expect![[r#"
        ID=lanzaboote
        PRETTY_NAME=LanzaOS (Generation 1, 1970-01-01)
        VERSION_ID=Generation 1, 1970-01-01
    "#]];

    expected.assert_eq(&String::from_utf8(os_release_section)?);

    Ok(())
}

#[test]
fn generate_expected_os_release_for_profile() -> Result<()> {
    let esp_mountpoint = tempdir()?;
    let tmpdir = tempdir()?;
    let profiles = tempdir()?;
    let toplevel = common::setup_toplevel(tmpdir.path())?;

    let generation_link = common::setup_profile_generation_link_from_toplevel(
        &toplevel,
        profiles.path(),
        Some("custom"),
        1,
    )
    .expect("Failed to setup generation link");

    let output0 = common::lanzaboote_install(0, esp_mountpoint.path(), vec![generation_link])?;
    assert!(output0.status.success());

    let stub_data = fs::read(common::profile_image_path(
        &esp_mountpoint,
        Some("custom"),
        1,
        &toplevel,
    )?)?;
    let os_release_section = pe_section(&stub_data, ".osrel")
        .context("Failed to read .osrelease PE section.")?
        .to_owned();

    let expected = expect![[r#"
        ID=lanzaboote
        PRETTY_NAME=LanzaOS [custom] (Generation 1, 1970-01-01)
        VERSION_ID=Generation 1, 1970-01-01
    "#]];

    expected.assert_eq(&String::from_utf8(os_release_section)?);

    Ok(())
}

/// A generation whose installed stub embeds an outdated os-release (e.g. because the label
/// changed while the toplevel, and with it the stub file name, stayed the same) must be
/// reinstalled so that the boot menu shows the correct data.
#[test]
fn reinstall_on_stale_os_release() -> Result<()> {
    let esp_mountpoint = tempdir()?;
    let tmpdir = tempdir()?;
    let profiles = tempdir()?;
    let toplevel = common::setup_toplevel(tmpdir.path())?;

    let generation_link =
        common::setup_generation_link_from_toplevel(&toplevel, profiles.path(), 1)?;
    let output0 =
        common::lanzaboote_install(0, esp_mountpoint.path(), vec![generation_link.clone()])?;
    assert!(output0.status.success());

    let read_pretty_name = || -> Result<String> {
        let stub_data = fs::read(common::image_path(&esp_mountpoint, 1, &toplevel)?)?;
        let os_release = pe_section(&stub_data, ".osrel").context("Failed to read .osrel.")?;
        let pretty_name = String::from_utf8(os_release.to_vec())?
            .lines()
            .find(|line| line.starts_with("PRETTY_NAME="))
            .context("Missing PRETTY_NAME.")?
            .trim_start_matches("PRETTY_NAME=")
            .to_string();
        Ok(pretty_name)
    };
    assert_eq!(read_pretty_name()?, "LanzaOS (Generation 1, 1970-01-01)");

    // Change the label of the generation without changing its toplevel, so that the stub
    // file name stays the same.
    let bootspec_path = generation_link.join("boot.json");
    let mut bootspec: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&bootspec_path)?)?;
    bootspec["org.nixos.bootspec.v1"]["label"] = serde_json::json!("LanzaOS 2");
    fs::write(&bootspec_path, serde_json::to_vec(&bootspec)?)?;

    let output1 = common::lanzaboote_install(0, esp_mountpoint.path(), vec![generation_link])?;
    assert!(output1.status.success());

    // The stub was reinstalled and now embeds the updated os-release.
    assert_eq!(read_pretty_name()?, "LanzaOS 2 (Generation 1, 1970-01-01)");

    Ok(())
}

fn pe_section<'a>(file_data: &'a [u8], section_name: &str) -> Option<&'a [u8]> {
    let pe_binary = goblin::pe::PE::parse(file_data).ok()?;

    pe_binary
        .sections
        .iter()
        .find(|s| s.name().unwrap() == section_name)
        .and_then(|s| {
            let section_start: usize = s.pointer_to_raw_data.try_into().ok()?;
            assert!(s.virtual_size <= s.size_of_raw_data);
            let section_end: usize = section_start + usize::try_from(s.virtual_size).ok()?;
            Some(&file_data[section_start..section_end])
        })
}
