#![cfg(windows)]

use du_core_contracts::scan_observation::{AnchoredDirectory, DirectoryObservations, ObservedKind};
use du_platform_os::windows_observation::open_verified;
use std::{
    env, fs,
    io::{self, Write},
    os::windows::ffi::OsStrExt,
    path::PathBuf,
    process::{Command, Output},
};
use windows::{
    Win32::Storage::FileSystem::{
        DeleteVolumeMountPointW, GetLogicalDrives, GetVolumeNameForVolumeMountPointW,
        SetVolumeMountPointW,
    },
    core::PCWSTR,
};

struct VhdFixture {
    temp: tempfile::TempDir,
    image: PathBuf,
    mount_path: PathBuf,
    junction_path: PathBuf,
    letter: u8,
    attach_attempted: bool,
    mount_attempted: bool,
    junction_attempted: bool,
}

#[allow(unsafe_code)]
impl VhdFixture {
    fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let temp_root = env::var_os("RUNNER_TEMP").ok_or("missing RUNNER_TEMP")?;
        let temp = tempfile::Builder::new()
            .prefix("diskunfold-f02-39-")
            .tempdir_in(temp_root)?;
        let image = temp.path().join("mountpoint.vhdx");
        let mount_path = temp.path().join("volume-root");
        let junction_path = temp.path().join("directory-junction");
        fs::create_dir(&mount_path)?;

        // SAFETY: GetLogicalDrives has no pointer arguments.
        let drives = unsafe { GetLogicalDrives() };
        if drives == 0 {
            return Err(io::Error::last_os_error().into());
        }
        let letter = (b'D'..=b'Z')
            .rev()
            .find(|letter| drives & (1 << u32::from(*letter - b'A')) == 0)
            .ok_or("no free drive letter for the isolated VHD")?;

        Ok(Self {
            temp,
            image,
            mount_path,
            junction_path,
            letter,
            attach_attempted: false,
            mount_attempted: false,
            junction_attempted: false,
        })
    }

    fn diskpart(&self, script: &str) -> io::Result<Output> {
        let script_path = self.temp.path().join("diskpart.txt");
        let mut file = fs::File::create(&script_path)?;
        file.write_all(script.as_bytes())?;
        file.flush()?;
        drop(file);
        Command::new("diskpart.exe")
            .args(["/s"])
            .arg(script_path)
            .output()
    }

    fn check_diskpart(output: Output) -> io::Result<()> {
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        if !output.status.success()
            || stdout.contains("Virtual Disk Service error")
            || stdout.contains("DiskPart has encountered an error")
            || stderr.contains("error")
        {
            return Err(io::Error::other(format!(
                "DiskPart failed ({}): {}{}",
                output.status,
                stdout.trim(),
                stderr.trim()
            )));
        }
        Ok(())
    }

    fn volume_name(&self) -> Result<Vec<u16>, Box<dyn std::error::Error>> {
        let root = [u16::from(self.letter), b':' as u16, b'\\' as u16, 0];
        let mut volume_name = [0u16; 64];
        // SAFETY: root is NUL-terminated and the output buffer is writable.
        unsafe {
            GetVolumeNameForVolumeMountPointW(PCWSTR(root.as_ptr()), &mut volume_name)?;
        }
        if !volume_name.contains(&0) {
            return Err("volume GUID path was not terminated".into());
        }
        Ok(volume_name.to_vec())
    }

    fn mount_path_wide(&self) -> Vec<u16> {
        let mut path = self
            .mount_path
            .as_os_str()
            .encode_wide()
            .collect::<Vec<_>>();
        if path.last() != Some(&(b'\\' as u16)) {
            path.push(b'\\' as u16);
        }
        path.push(0);
        path
    }

    fn exercise(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        let image = self.image.display();
        Self::check_diskpart(self.diskpart(&format!(
            "create vdisk file=\"{image}\" maximum=64 type=expandable\r\nexit\r\n"
        ))?)?;
        if !self.image.is_file() {
            return Err("DiskPart did not create the VHD".into());
        }
        self.attach_attempted = true;
        Self::check_diskpart(self.diskpart(&format!(
            "select vdisk file=\"{image}\"\r\nattach vdisk\r\nexit\r\n"
        ))?)?;
        Self::check_diskpart(self.diskpart(&format!(
            "select vdisk file=\"{image}\"\r\ncreate partition primary\r\nformat fs=ntfs quick label=DU39MOUNT\r\nassign letter={}\r\nexit\r\n",
            char::from(self.letter)
        ))?)?;

        let target_root = PathBuf::from(format!("{}:\\", char::from(self.letter)));
        {
            let _target_root = open_verified(&target_root).map_err(|error| format!("{error:?}"))?;
        }

        let junction = Command::new("cmd.exe")
            .args(["/D", "/C", "mklink", "/J"])
            .arg(&self.junction_path)
            .arg(&target_root)
            .output()?;
        if !junction.status.success() {
            self.junction_attempted = fs::symlink_metadata(&self.junction_path).is_ok();
            return Err(format!(
                "could not create temporary NTFS junction ({}): {}{}",
                junction.status,
                String::from_utf8_lossy(&junction.stdout).trim(),
                String::from_utf8_lossy(&junction.stderr).trim()
            )
            .into());
        }
        self.junction_attempted = true;

        let volume = self.volume_name()?;
        let mount_path = self.mount_path_wide();
        self.mount_attempted = true;
        // SAFETY: both arguments are NUL-terminated UTF-16 paths that remain live.
        unsafe {
            SetVolumeMountPointW(PCWSTR(mount_path.as_ptr()), PCWSTR(volume.as_ptr()))?;
        }

        let mut directory =
            open_verified(self.temp.path()).map_err(|error| format!("{error:?}"))?;
        let parent_volume = directory
            .identity()
            .map_err(|error| format!("{error:?}"))?
            .volume;
        let mut found = Vec::new();
        loop {
            let batch = directory
                .next_batch()
                .map_err(|error| format!("{error:?}"))?;
            if batch.is_empty() {
                break;
            }
            found.extend(batch);
        }
        let mount_name = "volume-root"
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>();
        let entry = found
            .iter()
            .find(|entry| entry.name == mount_name)
            .ok_or("mounted directory was not enumerated")?;
        assert_eq!(entry.enumerated_kind, ObservedKind::MountPoint);
        let metadata = entry
            .metadata
            .as_ref()
            .map_err(|error| format!("{error:?}"))?;
        assert_eq!(metadata.kind, ObservedKind::MountPoint);
        assert_eq!(metadata.device, parent_volume);
        assert_eq!(metadata.inode, entry.enumerated_inode);
        assert!(metadata.size.is_err());
        assert!(metadata.modified_at.is_none());
        assert!(metadata.native_modified_at.is_none());
        assert!(metadata.allocated.is_none());
        assert!(metadata.link_count.is_none());
        assert!(metadata.space_identity.is_none());
        assert!(directory.open_child(entry).is_err());

        let junction_name = "directory-junction"
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>();
        let entry = found
            .iter()
            .find(|entry| entry.name == junction_name)
            .ok_or("NTFS directory junction was not enumerated")?;
        assert_eq!(entry.enumerated_kind, ObservedKind::Link);
        let metadata = entry
            .metadata
            .as_ref()
            .map_err(|error| format!("{error:?}"))?;
        assert_eq!(metadata.kind, ObservedKind::Link);
        assert_eq!(metadata.device, parent_volume);
        assert_eq!(metadata.inode, entry.enumerated_inode);
        assert!(directory.open_child(entry).is_err());
        Ok(())
    }

    fn cleanup(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        let mut failure = None;
        if self.junction_attempted {
            if let Err(error) = fs::remove_dir(&self.junction_path) {
                failure = Some(format!("could not remove temporary NTFS junction: {error}"));
            } else {
                self.junction_attempted = false;
            }
        }
        if self.mount_attempted {
            let mount_path = self.mount_path_wide();
            // SAFETY: mount_path is NUL-terminated UTF-16 and remains live for the call.
            if let Err(error) = unsafe { DeleteVolumeMountPointW(PCWSTR(mount_path.as_ptr())) } {
                failure = Some(format!("could not remove temporary volume mount: {error}"));
            } else {
                self.mount_attempted = false;
            }
        }
        if self.attach_attempted {
            let image = self.image.display();
            let detached = self
                .diskpart(&format!(
                    "select vdisk file=\"{image}\"\r\ndetach vdisk\r\nexit\r\n"
                ))
                .and_then(Self::check_diskpart);
            if let Err(error) = detached {
                failure.get_or_insert_with(|| format!("could not detach temporary VHD: {error}"));
            } else {
                self.attach_attempted = false;
            }
        }
        if let Some(failure) = failure {
            Err(failure.into())
        } else {
            Ok(())
        }
    }
}

impl Drop for VhdFixture {
    fn drop(&mut self) {
        if let Err(error) = self.cleanup() {
            eprintln!("Windows mount-point fixture cleanup failed: {error}");
        }
    }
}

fn is_github_windows_runner() -> bool {
    env::var("GITHUB_ACTIONS").is_ok_and(|value| value == "true")
        && env::var("RUNNER_OS").is_ok_and(|value| value == "Windows")
        && env::var_os("RUNNER_TEMP").is_some()
}

#[test]
#[ignore = "requires an isolated GitHub Windows runner to create and attach a temporary VHD"]
fn volume_mount_point_uses_parent_identity_and_reports_no_file_statistics()
-> Result<(), Box<dyn std::error::Error>> {
    if !is_github_windows_runner() {
        eprintln!("skipping VHD fixture outside a GitHub Windows runner");
        return Ok(());
    }

    let mut fixture = VhdFixture::new()?;
    let result = fixture.exercise();
    let cleanup = fixture.cleanup();
    match (result, cleanup) {
        (Ok(()), Ok(())) => Ok(()),
        (Err(error), Ok(())) | (Ok(()), Err(error)) => Err(error),
        (Err(error), Err(cleanup)) => {
            Err(format!("{error}; cleanup also failed: {cleanup}").into())
        }
    }
}
