use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Result;
use tempfile::tempdir;

use crate::common::{self, count_files};

#[test]
fn keep_only_configured_number_of_generations() -> Result<()> {
    let esp_mountpoint = tempdir()?;
    let tmpdir = tempdir()?;
    let profiles = tempdir()?;
    let generation_links: Vec<PathBuf> = [1, 2, 3]
        .into_iter()
        .map(|v| {
            common::setup_generation_link(tmpdir.path(), profiles.path(), v)
                .expect("Failed to setup generation link")
        })
        .collect();
    let stub_count = || count_files(&esp_mountpoint.path().join("EFI/Linux")).unwrap();
    let kernel_and_initrd_count = || count_files(&esp_mountpoint.path().join("EFI/nixos")).unwrap();

    // Install all 3 generations.
    let output0 = common::lanzaboote_install(0, esp_mountpoint.path(), generation_links.clone())?;
    assert!(output0.status.success());
    assert_eq!(stub_count(), 6, "Wrong number of stubs after installation");
    assert_eq!(
        kernel_and_initrd_count(),
        2,
        "Wrong number of kernels & initrds after installation"
    );

    // Call `lanzatool install` again with a config limit of 2 and assert that one is deleted.
    // In addition, the garbage kernel should be deleted as well.
    let output1 = common::lanzaboote_install(2, esp_mountpoint.path(), generation_links)?;
    assert!(output1.status.success());
    assert_eq!(stub_count(), 4, "Wrong number of stubs after gc.");
    assert_eq!(
        kernel_and_initrd_count(),
        2,
        "Wrong number of kernels & initrds after gc."
    );

    Ok(())
}

#[test]
fn delete_garbage_kernel() -> Result<()> {
    let esp_mountpoint = tempdir()?;
    let tmpdir = tempdir()?;
    let profiles = tempdir()?;
    let generation_links: Vec<PathBuf> = [1, 2, 3]
        .into_iter()
        .map(|v| {
            common::setup_generation_link(tmpdir.path(), profiles.path(), v)
                .expect("Failed to setup generation link")
        })
        .collect();
    let stub_count = || count_files(&esp_mountpoint.path().join("EFI/Linux")).unwrap();
    let kernel_and_initrd_count = || count_files(&esp_mountpoint.path().join("EFI/nixos")).unwrap();

    // Install all 3 generations.
    let output0 = common::lanzaboote_install(0, esp_mountpoint.path(), generation_links.clone())?;
    assert!(output0.status.success());

    // Create a garbage kernel, which should be deleted.
    fs::write(
        esp_mountpoint.path().join("EFI/nixos/kernel-garbage.efi"),
        "garbage",
    )?;

    // Call `lanzatool install` again with a config limit of 2.
    // In addition, the garbage kernel should be deleted as well.
    let output1 = common::lanzaboote_install(2, esp_mountpoint.path(), generation_links)?;
    assert!(output1.status.success());

    assert_eq!(stub_count(), 4, "Wrong number of stubs after gc.");
    assert_eq!(
        kernel_and_initrd_count(),
        2,
        "Wrong number of kernels & initrds after gc."
    );

    Ok(())
}

#[test]
fn keep_unrelated_files_on_esp() -> Result<()> {
    let esp_mountpoint = tempdir()?;
    let tmpdir = tempdir()?;
    let profiles = tempdir()?;
    let generation_links: Vec<PathBuf> = [1, 2, 3]
        .into_iter()
        .map(|v| {
            common::setup_generation_link(tmpdir.path(), profiles.path(), v)
                .expect("Failed to setup generation link")
        })
        .collect();

    // Install all 3 generations.
    let output0 = common::lanzaboote_install(0, esp_mountpoint.path(), generation_links.clone())?;
    assert!(output0.status.success());

    let unrelated_loader_config = esp_mountpoint.path().join("loader/loader.conf");
    let unrelated_uki = esp_mountpoint.path().join("EFI/Linux/ubuntu.efi");
    let unrelated_os = esp_mountpoint.path().join("EFI/windows");
    let unrelated_firmware = esp_mountpoint.path().join("dell");
    fs::File::create(&unrelated_loader_config)?;
    fs::File::create(&unrelated_uki)?;
    fs::create_dir(&unrelated_os)?;
    fs::create_dir(&unrelated_firmware)?;

    // Call `lanzatool install` again with a config limit of 2.
    let output1 = common::lanzaboote_install(2, esp_mountpoint.path(), generation_links)?;
    assert!(output1.status.success());

    assert!(unrelated_loader_config.exists());
    assert!(unrelated_uki.exists());
    assert!(unrelated_os.exists());
    assert!(unrelated_firmware.exists());

    Ok(())
}

fn find_file_with_prefix(path: impl AsRef<Path>, prefix: &str) {
    let found = fs::read_dir(path)
        .expect("Directory doesn't exist")
        .find(|entry| {
            entry.as_ref().is_ok_and(|e| {
                e.path()
                    .file_name()
                    .is_some_and(|filename| filename.to_string_lossy().starts_with(prefix))
            })
        });
    assert!(found.is_some_and(|e| e.is_ok()));
}

#[test]
fn keep_protected_boot() -> Result<()> {
    let esp_mountpoint = tempdir()?;
    let tmpdir = tempdir()?;
    let profiles = tempdir()?;
    let mut generation_links: Vec<PathBuf> = [1, 2, 3]
        .into_iter()
        .map(|v| {
            common::setup_generation_link(tmpdir.path(), profiles.path(), v)
                .expect("Failed to setup generation link")
        })
        .collect();
    let stub_count = || count_files(&esp_mountpoint.path().join("EFI/Linux")).unwrap();
    let kernel_and_initrd_count = || count_files(&esp_mountpoint.path().join("EFI/nixos")).unwrap();

    // Install all 3 generations.
    let output0 =
        common::Lanzaboote::new().install(esp_mountpoint.path(), generation_links.clone())?;
    assert!(output0.status.success());
    assert_eq!(stub_count(), 6, "Wrong number of stubs after installation");
    assert_eq!(
        kernel_and_initrd_count(),
        2,
        "Wrong number of kernels & initrds after installation"
    );

    let uki_dir = esp_mountpoint.path().join("EFI/Linux");
    find_file_with_prefix(&uki_dir, "nixos-generation-1");
    find_file_with_prefix(&uki_dir, "nixos-generation-2");
    find_file_with_prefix(&uki_dir, "nixos-generation-3");

    // Add a new generation
    generation_links.push(
        common::setup_generation_link(tmpdir.path(), profiles.path(), 4)
            .expect("Failed to setup generation link"),
    );

    // Call `lanzatool install` again with a config limit of 2 and assert that one is deleted.
    // In addition, the garbage kernel should be deleted as well.
    let output1 = common::Lanzaboote::new()
        .config_limit(3)
        .protected_system(generation_links[0].clone())
        .install(esp_mountpoint.path(), generation_links)?;
    assert!(output1.status.success());
    assert_eq!(stub_count(), 6, "Wrong number of stubs after gc.");
    assert_eq!(
        kernel_and_initrd_count(),
        2,
        "Wrong number of kernels & initrds after gc."
    );

    let uki_dir = esp_mountpoint.path().join("EFI/Linux");
    // Note how 1 is preserved because it's the booted version
    find_file_with_prefix(&uki_dir, "nixos-generation-1");
    // Note how 2 is missing here
    find_file_with_prefix(&uki_dir, "nixos-generation-3");
    find_file_with_prefix(&uki_dir, "nixos-generation-4");

    Ok(())
}

fn refute_file_with_prefix(path: impl AsRef<Path>, prefix: &str) {
    let found = fs::read_dir(path)
        .expect("Directory doesn't exist")
        .any(|entry| {
            entry.is_ok_and(|e| {
                e.path()
                    .file_name()
                    .is_some_and(|filename| filename.to_string_lossy().starts_with(prefix))
            })
        });
    assert!(!found, "No file with prefix '{prefix}' expected");
}

/// When a specialisation is booted, `/run/booted-system` points at the specialisation's
/// toplevel instead of the parent toplevel the generation link points at. The generation
/// providing the booted specialisation must still be protected.
#[test]
fn keep_protected_boot_when_booted_into_specialisation() -> Result<()> {
    let esp_mountpoint = tempdir()?;
    let tmpdir = tempdir()?;
    let profiles = tempdir()?;
    let toplevel = common::setup_toplevel(tmpdir.path())?;
    let generation_links: Vec<PathBuf> = vec![
        common::setup_generation_link_from_toplevel(&toplevel, profiles.path(), 1)?,
        common::setup_generation_link(tmpdir.path(), profiles.path(), 2)?,
        common::setup_generation_link(tmpdir.path(), profiles.path(), 3)?,
    ];

    // Call `lanzatool install` with a config limit of 2 while booted into the specialisation
    // of generation 1. The limit alone would keep generations 2 and 3.
    let output = common::Lanzaboote::new()
        .config_limit(2)
        .protected_system(toplevel.join("specialisation/rescue"))
        .install(esp_mountpoint.path(), generation_links)?;
    assert!(output.status.success());

    let uki_dir = esp_mountpoint.path().join("EFI/Linux");
    // Generation 1 is preserved although only its specialisation is booted.
    find_file_with_prefix(&uki_dir, "nixos-generation-1");
    refute_file_with_prefix(&uki_dir, "nixos-generation-2");
    find_file_with_prefix(&uki_dir, "nixos-generation-3");

    Ok(())
}

/// With a configuration limit of one, making room for the booted generation must not evict
/// the generation that was just built: both are kept so that the system can still be rolled
/// forward into the newest generation.
#[test]
fn keep_booted_and_newest_generation_when_limit_is_one() -> Result<()> {
    let esp_mountpoint = tempdir()?;
    let tmpdir = tempdir()?;
    let profiles = tempdir()?;
    let generation_links: Vec<PathBuf> = vec![
        common::setup_generation_link(tmpdir.path(), profiles.path(), 1)?,
        common::setup_generation_link(tmpdir.path(), profiles.path(), 2)?,
    ];

    let output = common::Lanzaboote::new()
        .config_limit(1)
        .protected_system(generation_links[0].clone())
        .install(esp_mountpoint.path(), generation_links)?;
    assert!(output.status.success());

    let uki_dir = esp_mountpoint.path().join("EFI/Linux");
    find_file_with_prefix(&uki_dir, "nixos-generation-1");
    find_file_with_prefix(&uki_dir, "nixos-generation-2");

    Ok(())
}

/// A malformed generation that falls outside the configuration limit must still be recorded,
/// so that garbage collection stays disabled and its installed artifacts survive.
#[test]
fn malformed_generation_outside_limit_disables_gc() -> Result<()> {
    let esp_mountpoint = tempdir()?;
    let tmpdir = tempdir()?;
    let profiles = tempdir()?;
    let generation_links: Vec<PathBuf> = vec![
        common::setup_generation_link(tmpdir.path(), profiles.path(), 1)?,
        common::setup_generation_link(tmpdir.path(), profiles.path(), 2)?,
        common::setup_generation_link(tmpdir.path(), profiles.path(), 3)?,
    ];

    // Install all generations without a limit.
    let output0 = common::lanzaboote_install(0, esp_mountpoint.path(), generation_links.clone())?;
    assert!(output0.status.success());

    // Turn generation 1 into a malformed generation without a bootspec.
    fs::remove_file(generation_links[0].join("boot.json"))?;

    // With a limit of 2, generation 1 falls outside the retained set.
    let output1 = common::lanzaboote_install(2, esp_mountpoint.path(), generation_links)?;
    assert!(output1.status.success());

    // Garbage collection is disabled by the malformed generation, so the artifacts
    // installed for it earlier survive.
    let uki_dir = esp_mountpoint.path().join("EFI/Linux");
    find_file_with_prefix(&uki_dir, "nixos-generation-1");
    find_file_with_prefix(&uki_dir, "nixos-generation-3");
    let stderr = String::from_utf8(output1.stderr)?;
    assert!(
        stderr.contains("Garbage collection is disabled"),
        "{stderr}"
    );

    Ok(())
}
