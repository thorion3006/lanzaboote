use std::fs;
use std::process::Command;
use std::str::FromStr;

use anyhow::{Context, Result};
use base32ct::{Base32Unpadded, Encoding};
use lanzaboote_tool::os_release::OsRelease;
use tempfile::{NamedTempFile, tempdir};

use crate::common::{
    self, count_files, hash_file, remove_signature, setup_generation_link_from_toplevel,
    verify_signature,
};

/// Install two generations that point at the same toplevel.
/// This should install two lanzaboote images and one kernel and one initrd.
#[test]
fn do_not_install_duplicates() -> Result<()> {
    for profile in [None, Some("MyProfile")] {
        let esp = tempdir()?;
        let tmpdir = tempdir()?;
        let profiles = tempdir()?;
        let toplevel = common::setup_toplevel(tmpdir.path())?;

        let generation_link1 =
            setup_generation_link_from_toplevel(&toplevel, profiles.path(), 1, profile)?;
        let generation_link2 =
            setup_generation_link_from_toplevel(&toplevel, profiles.path(), 2, profile)?;
        let generation_links = vec![generation_link1, generation_link2];

        let stub_count = || count_files(&esp.path().join("EFI/Linux")).unwrap();
        let kernel_and_initrd_count = || count_files(&esp.path().join("EFI/nixos")).unwrap();

        let output1 = common::lanzaboote_install(0, esp.path(), generation_links)?;
        assert!(output1.status.success());
        assert_eq!(stub_count(), 4, "Wrong number of stubs after installation");
        assert_eq!(
            kernel_and_initrd_count(),
            2,
            "Wrong number of kernels & initrds after installation"
        );
    }
    Ok(())
}

#[test]
fn do_not_overwrite_images() -> Result<()> {
    for profile in [None, Some("MyProfile")] {
        let esp = tempdir()?;
        let tmpdir = tempdir()?;
        let profiles = tempdir()?;
        let toplevel = common::setup_toplevel(tmpdir.path())?;

        let image1 = common::image_path(&esp, 1, profile, profile.is_none(), &toplevel)?;
        let image2 = common::image_path(&esp, 2, profile, profile.is_none(), &toplevel)?;

        let generation_link1 =
            setup_generation_link_from_toplevel(&toplevel, profiles.path(), 1, profile)?;
        let generation_link2 =
            setup_generation_link_from_toplevel(&toplevel, profiles.path(), 2, profile)?;
        let generation_links = vec![generation_link1, generation_link2];

        let output1 = common::lanzaboote_install(0, esp.path(), generation_links.clone())?;
        assert!(output1.status.success());

        assert!(verify_signature(&image1)?);
        remove_signature(&image1)?;
        assert!(!verify_signature(&image1)?);
        assert!(verify_signature(&image2)?);

        let output2 = common::lanzaboote_install(0, esp.path(), generation_links)?;
        assert!(output2.status.success());

        assert!(!verify_signature(&image1)?);
        assert!(verify_signature(&image2)?);
    }

    Ok(())
}

#[test]
fn detect_generation_number_reuse() -> Result<()> {
    for profile in [None, Some("MyProfile")] {
        let esp = tempdir()?;
        let tmpdir = tempdir()?;
        let profiles = tempdir()?;
        let toplevel1 = common::setup_toplevel(tmpdir.path())?;
        let toplevel2 = common::setup_toplevel(tmpdir.path())?;

        let image1 = common::image_path(&esp, 1, profile, profile.is_none(), &toplevel1)?;
        // this deliberately gets the same number!
        let image2 = common::image_path(&esp, 1, profile, profile.is_none(), &toplevel2)?;

        let generation_link1 =
            setup_generation_link_from_toplevel(&toplevel1, profiles.path(), 1, profile)?;
        let output1 = common::lanzaboote_install(0, esp.path(), vec![generation_link1])?;
        assert!(output1.status.success());
        assert!(image1.exists());
        assert!(!image2.exists());

        let link_path = match profile {
            Some(profile) => profiles
                .path()
                .join(format!("system-profiles/{profile}-1-link")),
            None => profiles.path().join("system-1-link"),
        };
        std::fs::remove_dir_all(link_path)?;
        let generation_link2 =
            setup_generation_link_from_toplevel(&toplevel2, profiles.path(), 1, profile)?;
        let output2 = common::lanzaboote_install(0, esp.path(), vec![generation_link2])?;
        assert!(output2.status.success());
        assert!(!image1.exists());
        assert!(image2.exists());
    }

    Ok(())
}

#[test]
fn content_addressing_works() -> Result<()> {
    let esp = tempdir()?;
    let tmpdir = tempdir()?;
    let profiles = tempdir()?;
    let toplevel = common::setup_toplevel(tmpdir.path())?;

    let generation_link = setup_generation_link_from_toplevel(&toplevel, profiles.path(), 1, None)?;
    let generation_links = vec![generation_link];

    let kernel_hash_source =
        hash_file(&toplevel.join("eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee-6.1.1/kernel"));

    let output0 = common::lanzaboote_install(1, esp.path(), generation_links)?;
    assert!(output0.status.success());

    let kernel_path = esp.path().join(format!(
        "EFI/nixos/kernel-6.1.1-{}.efi",
        Base32Unpadded::encode_string(&kernel_hash_source)
    ));

    // Implicitly assert that the content-addressed file actually exists.
    let kernel_hash = hash_file(&kernel_path);
    // Assert the written kernel is the source kernel.
    assert_eq!(kernel_hash_source, kernel_hash);

    Ok(())
}

#[test]
fn escape_profile_name_when_matching_existing_stubs() -> Result<()> {
    let esp = tempdir()?;
    let tmpdir = tempdir()?;
    let profiles = tempdir()?;
    let toplevel = common::setup_toplevel(tmpdir.path())?;
    let profile = "My W#@cky_Yet_L3g!t profile-name -3 [dev](1)+?";

    let image = common::image_path(&esp, 1, Some(profile), false, &toplevel)?;
    let generation_link =
        setup_generation_link_from_toplevel(&toplevel, profiles.path(), 1, Some(profile))?;

    let output1 = common::lanzaboote_install(0, esp.path(), vec![&generation_link])?;
    assert!(output1.status.success());
    assert!(image.exists());

    remove_signature(&image)?;
    assert!(!verify_signature(&image)?);

    let output2 = common::lanzaboote_install(0, esp.path(), vec![generation_link])?;
    assert!(output2.status.success());
    assert!(!verify_signature(&image)?);

    Ok(())
}

#[test]
fn keep_custom_system_profile_distinct_from_default_profile() -> Result<()> {
    let esp = tempdir()?;
    let tmpdir = tempdir()?;
    let profiles = tempdir()?;
    let default_toplevel = common::setup_toplevel(tmpdir.path())?;
    let custom_toplevel = common::setup_toplevel(tmpdir.path())?;

    let default_image = common::image_path(&esp, 1, None, true, &default_toplevel)?;
    let custom_image = common::image_path(&esp, 1, Some("system"), false, &custom_toplevel)?;

    let default_generation =
        setup_generation_link_from_toplevel(&default_toplevel, profiles.path(), 1, None)?;
    let custom_generation =
        setup_generation_link_from_toplevel(&custom_toplevel, profiles.path(), 1, Some("system"))?;

    let output =
        common::lanzaboote_install(0, esp.path(), vec![default_generation, custom_generation])?;
    assert!(output.status.success());
    assert!(default_image.exists());
    assert!(custom_image.exists());
    assert_ne!(default_image, custom_image);

    Ok(())
}

#[test]
fn rewrite_installed_generation_when_os_release_is_stale() -> Result<()> {
    let esp = tempdir()?;
    let tmpdir = tempdir()?;
    let profiles = tempdir()?;
    let toplevel = common::setup_toplevel(tmpdir.path())?;

    let generation_link = setup_generation_link_from_toplevel(&toplevel, profiles.path(), 1, None)?;
    let image = common::image_path(&esp, 1, None, true, &toplevel)?;

    let output1 = common::lanzaboote_install(0, esp.path(), vec![generation_link.clone()])?;
    assert!(output1.status.success());
    assert!(verify_signature(&image)?);

    rewrite_os_release_section(
        &image,
        "ID=lanzaboote\nPRETTY_NAME=LanzaOS (Generation 1, 1970-01-01)\nVERSION_ID=Generation 1, 1970-01-01\n",
    )?;
    assert!(!verify_signature(&image)?);

    let output2 = common::lanzaboote_install(0, esp.path(), vec![generation_link])?;
    assert!(output2.status.success());
    assert!(verify_signature(&image)?);

    let os_release = read_os_release(&image)?;
    assert_eq!(os_release.0["VERSION_ID"], "19700101000000-generation-1");

    Ok(())
}

fn rewrite_os_release_section(image: &std::path::Path, contents: &str) -> Result<()> {
    let os_release = NamedTempFile::new()?;
    fs::write(os_release.path(), contents)?;

    let rewritten = image.with_extension("efi.tmp");
    let status = Command::new("objcopy")
        .arg("--update-section")
        .arg(format!(".osrel={}", os_release.path().display()))
        .arg(image)
        .arg(&rewritten)
        .status()?;
    assert!(status.success());

    fs::rename(&rewritten, image)?;
    Ok(())
}

fn read_os_release(image: &std::path::Path) -> Result<OsRelease> {
    let file = fs::read(image)?;
    let os_release_section =
        common::pe_section(&file, ".osrel").context("Failed to read .osrelease PE section.")?;
    Ok(OsRelease::from_str(
        std::str::from_utf8(os_release_section)?.trim_end_matches('\0'),
    )?)
}
