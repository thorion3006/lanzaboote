use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use tempfile::{TempDir, tempdir};

use crate::common::{self, profile_image_path, setup_profile_generation_link_from_toplevel};

const CONFIGURED_LOADER_CONFIG: &str = "timeout 0\nconsole-mode 1\ndefault nixos-*\n";

fn read_loader_config(esp: &TempDir) -> String {
    fs::read_to_string(esp.path().join("loader/loader.conf")).expect("Failed to read loader.conf")
}

fn file_name(path: &Path) -> String {
    path.file_name().unwrap().to_string_lossy().into_owned()
}

#[test]
fn keep_configured_default_without_default_system() -> Result<()> {
    let esp = tempdir()?;
    let tmpdir = tempdir()?;
    let profiles = tempdir()?;
    let generation_link = common::setup_generation_link(tmpdir.path(), profiles.path(), 1)?;

    let output = common::lanzaboote_install(0, esp.path(), [generation_link])?;
    assert!(output.status.success());
    assert_eq!(read_loader_config(&esp), CONFIGURED_LOADER_CONFIG);

    Ok(())
}

/// The default system becomes the default entry, even if it is not the newest generation and
/// belongs to another profile.
#[test]
fn default_system_becomes_default_entry() -> Result<()> {
    let esp = tempdir()?;
    let tmpdir = tempdir()?;
    let profiles = tempdir()?;
    let toplevel1 = common::setup_toplevel(tmpdir.path())?;
    let toplevel2 = common::setup_toplevel(tmpdir.path())?;
    let custom_toplevel = common::setup_toplevel(tmpdir.path())?;

    let generation_links = [
        setup_profile_generation_link_from_toplevel(&toplevel1, profiles.path(), None, 1)?,
        setup_profile_generation_link_from_toplevel(&toplevel2, profiles.path(), None, 40)?,
        setup_profile_generation_link_from_toplevel(
            &custom_toplevel,
            profiles.path(),
            Some("custom"),
            3,
        )?,
    ];

    let output0 = common::Lanzaboote::new()
        .default_system(custom_toplevel.clone())
        .install(esp.path(), &generation_links)?;
    assert!(output0.status.success());
    let custom_image = profile_image_path(&esp, Some("custom"), 3, &custom_toplevel)?;
    assert_eq!(
        read_loader_config(&esp),
        format!(
            "timeout 0\nconsole-mode 1\ndefault {}\n",
            file_name(&custom_image)
        )
    );

    let output1 = common::Lanzaboote::new()
        .default_system(toplevel1.clone())
        .install(esp.path(), &generation_links)?;
    assert!(output1.status.success());
    let image1 = profile_image_path(&esp, None, 1, &toplevel1)?;
    assert_eq!(
        read_loader_config(&esp),
        format!(
            "timeout 0\nconsole-mode 1\ndefault {}\n",
            file_name(&image1)
        )
    );

    Ok(())
}

#[test]
fn default_system_can_be_a_specialisation() -> Result<()> {
    let esp = tempdir()?;
    let tmpdir = tempdir()?;
    let profiles = tempdir()?;
    let toplevel = common::setup_toplevel(tmpdir.path())?;
    let generation_link =
        setup_profile_generation_link_from_toplevel(&toplevel, profiles.path(), Some("custom"), 1)?;

    let output = common::Lanzaboote::new()
        .default_system(toplevel.join("specialisation/rescue"))
        .install(esp.path(), [generation_link])?;
    assert!(output.status.success());

    let specialisation_stub = fs::read_dir(esp.path().join("EFI/Linux"))?
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .find(|name| {
            name.starts_with(&format!(
                "nixos-profile-{}-generation-1-specialisation-rescue-",
                lanzaboote_tool::generation::profile_hash_component("custom")
            ))
        })
        .context("Specialisation stub is missing")?;
    assert_eq!(
        read_loader_config(&esp),
        format!("timeout 0\nconsole-mode 1\ndefault {specialisation_stub}\n")
    );

    Ok(())
}

/// If several generations point to the default system, the newest one becomes the default.
#[test]
fn default_system_prefers_newest_generation() -> Result<()> {
    let esp = tempdir()?;
    let tmpdir = tempdir()?;
    let profiles = tempdir()?;
    let toplevel = common::setup_toplevel(tmpdir.path())?;
    let generation_links = [
        setup_profile_generation_link_from_toplevel(&toplevel, profiles.path(), None, 1)?,
        setup_profile_generation_link_from_toplevel(&toplevel, profiles.path(), None, 2)?,
    ];

    let output = common::Lanzaboote::new()
        .default_system(toplevel.clone())
        .install(esp.path(), &generation_links)?;
    assert!(output.status.success());

    let image2 = profile_image_path(&esp, None, 2, &toplevel)?;
    assert_eq!(
        read_loader_config(&esp),
        format!(
            "timeout 0\nconsole-mode 1\ndefault {}\n",
            file_name(&image2)
        )
    );

    Ok(())
}

/// Switching to an older generation that falls outside the configuration limit must still
/// retain it and make it the default entry, so that the switch actually changes the next
/// boot.
#[test]
fn default_system_outside_limit_is_retained() -> Result<()> {
    let esp = tempdir()?;
    let tmpdir = tempdir()?;
    let profiles = tempdir()?;
    let toplevel1 = common::setup_toplevel(tmpdir.path())?;
    let toplevel2 = common::setup_toplevel(tmpdir.path())?;
    let toplevel3 = common::setup_toplevel(tmpdir.path())?;
    let generation_links = [
        setup_profile_generation_link_from_toplevel(&toplevel1, profiles.path(), None, 1)?,
        setup_profile_generation_link_from_toplevel(&toplevel2, profiles.path(), None, 2)?,
        setup_profile_generation_link_from_toplevel(&toplevel3, profiles.path(), None, 3)?,
    ];

    // Generation 3 is booted, generation 2 is being switched to, the limit keeps only the
    // newest generation.
    let output = common::Lanzaboote::new()
        .config_limit(1)
        .protected_system(generation_links[2].clone())
        .default_system(toplevel2.clone())
        .install(esp.path(), &generation_links)?;
    assert!(output.status.success());

    let image2 = profile_image_path(&esp, None, 2, &toplevel2)?;
    assert!(image2.exists(), "{image2:?}");
    assert_eq!(
        read_loader_config(&esp),
        format!(
            "timeout 0\nconsole-mode 1\ndefault {}\n",
            file_name(&image2)
        )
    );

    Ok(())
}

/// When both the booted system and the requested default system fall outside the
/// configuration limit, making room for the default system must not evict the booted
/// system protected by `protectedSystem`.
#[test]
fn default_system_retention_does_not_evict_protected_system() -> Result<()> {
    let esp = tempdir()?;
    let tmpdir = tempdir()?;
    let profiles = tempdir()?;
    let mut generation_links = Vec::new();
    for version in 1..=4 {
        let toplevel = common::setup_toplevel(tmpdir.path())?;
        let link =
            setup_profile_generation_link_from_toplevel(&toplevel, profiles.path(), None, version)?;
        generation_links.push((link, toplevel));
    }

    // Generation 1 is booted, generation 2 is being switched to, the limit keeps the two
    // newest generations (3 and 4).
    let output = common::Lanzaboote::new()
        .config_limit(2)
        .protected_system(generation_links[0].0.clone())
        .default_system(generation_links[1].1.clone())
        .install(
            esp.path(),
            generation_links.iter().map(|(link, _)| link.clone()),
        )?;
    assert!(output.status.success());

    let uki_dir = esp.path().join("EFI/Linux");
    // The protected booted system and the requested default system both survive.
    assert!(
        profile_image_path(&esp, None, 1, &generation_links[0].1)?.exists(),
        "protected generation 1 is missing"
    );
    let image2 = profile_image_path(&esp, None, 2, &generation_links[1].1)?;
    assert!(image2.exists(), "{image2:?}");
    assert_eq!(
        read_loader_config(&esp),
        format!(
            "timeout 0\nconsole-mode 1\ndefault {}\n",
            file_name(&image2)
        )
    );
    // Room was made among the unprotected generations.
    assert!(!profile_image_path(&esp, None, 3, &generation_links[2].1)?.exists());
    assert!(!profile_image_path(&esp, None, 4, &generation_links[3].1)?.exists());

    Ok(())
}

/// With boot counting, the default system becomes the preferred entry so that systemd-boot falls
/// back to the configured default once it has run out of tries.
#[test]
fn default_system_is_preferred_with_boot_counting() -> Result<()> {
    let esp = tempdir()?;
    let tmpdir = tempdir()?;
    let profiles = tempdir()?;
    let toplevel = common::setup_toplevel(tmpdir.path())?;
    let generation_link =
        setup_profile_generation_link_from_toplevel(&toplevel, profiles.path(), Some("custom"), 1)?;

    let output = common::Lanzaboote::new()
        .bootcounting_initial_tries(3)
        .default_system(toplevel.clone())
        .install(esp.path(), [generation_link])?;
    assert!(output.status.success());

    // The stub has a boot counter, but the entry ID does not contain it.
    let image = profile_image_path(&esp, Some("custom"), 1, &toplevel)?;
    assert!(image_with_counter(&image, 3).exists());
    assert_eq!(
        read_loader_config(&esp),
        format!(
            "{CONFIGURED_LOADER_CONFIG}preferred {}\n",
            file_name(&image)
        )
    );

    Ok(())
}

#[test]
fn keep_configured_default_if_default_system_is_not_installed() -> Result<()> {
    let esp = tempdir()?;
    let tmpdir = tempdir()?;
    let profiles = tempdir()?;
    let generation_link = common::setup_generation_link(tmpdir.path(), profiles.path(), 1)?;
    let unknown_toplevel = common::setup_toplevel(tmpdir.path())?;

    let output = common::Lanzaboote::new()
        .default_system(unknown_toplevel)
        .install(esp.path(), [generation_link])?;
    assert!(output.status.success());
    assert_eq!(read_loader_config(&esp), CONFIGURED_LOADER_CONFIG);

    Ok(())
}

fn image_with_counter(image: &Path, tries: u32) -> PathBuf {
    image.with_file_name(format!(
        "{}+{tries}.efi",
        image.file_stem().unwrap().to_string_lossy()
    ))
}
