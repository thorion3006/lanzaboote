use std::fs;
use std::str::FromStr;

use anyhow::{Context, Result};
use expect_test::{Expect, expect};
use lanzaboote_tool::os_release::OsRelease;
use tempfile::tempdir;

use crate::common;

#[test]
fn generate_expected_os_release() -> Result<()> {
    let esp_mountpoint = tempdir()?;
    let tmpdir = tempdir()?;
    let profiles = tempdir()?;
    let toplevel = common::setup_toplevel(tmpdir.path())?;

    let assert_os_release =
        |profile: Option<&str>, is_default_profile: bool, expected: Expect| -> Result<()> {
            let generation_link =
                common::setup_generation_link_from_toplevel(&toplevel, profiles.path(), 1, profile)
                    .expect("Failed to setup generation link");

            let output0 =
                common::lanzaboote_install(0, esp_mountpoint.path(), vec![generation_link])?;
            assert!(output0.status.success());

            let stub_data = fs::read(common::image_path(
                &esp_mountpoint,
                1,
                profile,
                is_default_profile,
                &toplevel,
            )?)?;
            let os_release_section = common::pe_section(&stub_data, ".osrel")
                .context("Failed to read .osrelease PE section.")?
                .to_owned();

            expected.assert_eq(&String::from_utf8(os_release_section)?);
            Ok(())
        };

    assert_os_release(
        None,
        true,
        expect![[r#"
            ID=lanzaboote
            PRETTY_NAME=LanzaOS (Generation 1, 1970-01-01)
            VERSION_ID=19700101000000-generation-1
        "#]],
    )?;

    assert_os_release(
        Some("My W#@cky_Yet_L3g!t profile-name -3"),
        false,
        expect![[r#"
            ID=lanzaboote
            PRETTY_NAME=LanzaOS [My W#@cky_Yet_L3g!t profile-name -3] (Generation 1, 1970-01-01)
            VERSION_ID=19700101000000-generation-1
        "#]],
    )?;

    assert_os_release(
        Some("system"),
        false,
        expect![[r#"
            ID=lanzaboote
            PRETTY_NAME=LanzaOS [system] (Generation 1, 1970-01-01)
            VERSION_ID=19700101000000-generation-1
        "#]],
    )?;

    Ok(())
}

#[test]
fn newer_profile_generation_gets_higher_version_id() -> Result<()> {
    let esp_mountpoint = tempdir()?;
    let tmpdir = tempdir()?;
    let profiles = tempdir()?;

    let default_toplevel = common::setup_toplevel(tmpdir.path())?;
    let default_generation =
        common::setup_generation_link_from_toplevel(&default_toplevel, profiles.path(), 46, None)?;
    common::set_generation_link_mtime(&default_generation, 0)?;

    let comin_toplevel = common::setup_toplevel(tmpdir.path())?;
    let comin_generation = common::setup_generation_link_from_toplevel(
        &comin_toplevel,
        profiles.path(),
        1,
        Some("comin"),
    )?;
    common::set_generation_link_mtime(&comin_generation, 10)?;

    let output = common::lanzaboote_install(
        0,
        esp_mountpoint.path(),
        vec![default_generation, comin_generation],
    )?;
    assert!(output.status.success());

    let default_os_release =
        os_release_for_image(&esp_mountpoint, 46, None, true, &default_toplevel)?;
    let comin_os_release =
        os_release_for_image(&esp_mountpoint, 1, Some("comin"), false, &comin_toplevel)?;

    assert!(
        default_os_release.0["VERSION_ID"] < comin_os_release.0["VERSION_ID"],
        "{} should sort before {}",
        default_os_release.0["VERSION_ID"],
        comin_os_release.0["VERSION_ID"]
    );

    Ok(())
}

fn os_release_for_image(
    esp: &tempfile::TempDir,
    version: u64,
    profile: Option<&str>,
    is_default_profile: bool,
    toplevel: &std::path::Path,
) -> Result<OsRelease> {
    let stub_data = fs::read(common::image_path(
        esp,
        version,
        profile,
        is_default_profile,
        toplevel,
    )?)?;
    let os_release_section = common::pe_section(&stub_data, ".osrel")
        .context("Failed to read .osrelease PE section.")?;
    let os_release_section = std::str::from_utf8(os_release_section)?;
    Ok(OsRelease::from_str(
        os_release_section.trim_end_matches('\0'),
    )?)
}
