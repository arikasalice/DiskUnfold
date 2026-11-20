//! Linux sysfs-based storage device observations.
use super::{
    DeviceProbe, OS_DEVICE_MULTIPLE_DISKS, OS_QUERY_FAILED, OS_UNSUPPORTED, ProbedDeviceKind,
};
use std::{
    fs, io,
    path::{Path, PathBuf},
};

/// Probes the device numbers obtained from an open root directory's metadata.
#[cfg(target_os = "linux")]
pub fn probe_dev(major: u32, minor: u32) -> DeviceProbe {
    probe_dev_in(Path::new("/sys"), major, minor)
}

/// Probes one device through a supplied sysfs root, for deterministic fixture tests.
pub(crate) fn probe_dev_in(sysfs: &Path, major: u32, minor: u32) -> DeviceProbe {
    probe_dev_with(sysfs, major, minor, &|path| fs::canonicalize(path))
}

fn probe_dev_with<F>(sysfs: &Path, major: u32, minor: u32, resolve: &F) -> DeviceProbe
where
    F: Fn(&Path) -> io::Result<PathBuf>,
{
    if major == 0 {
        return probe(ProbedDeviceKind::Unknown, None, Some(OS_UNSUPPORTED));
    }
    let entry = sysfs.join("dev/block").join(format!("{major}:{minor}"));
    let device = match resolve(&entry) {
        Ok(path) => path,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return probe(ProbedDeviceKind::Unknown, None, None);
        }
        Err(_) => return probe(ProbedDeviceKind::Unknown, None, Some(OS_QUERY_FAILED)),
    };

    let whole = match whole_disk(&device) {
        Ok(path) => path,
        Err(()) => return probe(ProbedDeviceKind::Unknown, None, Some(OS_QUERY_FAILED)),
    };
    let slaves = match slave_entries(&whole) {
        Ok(entries) => entries,
        Err(()) => return probe(ProbedDeviceKind::Unknown, None, Some(OS_QUERY_FAILED)),
    };
    let whole = match slaves.as_slice() {
        [] => whole,
        [slave] => {
            let Ok(slave_device) = resolve(slave) else {
                return probe(ProbedDeviceKind::Unknown, None, Some(OS_QUERY_FAILED));
            };
            match whole_disk(&slave_device) {
                Ok(path) => path,
                Err(()) => return probe(ProbedDeviceKind::Unknown, None, Some(OS_QUERY_FAILED)),
            }
        }
        _ => {
            return probe(
                ProbedDeviceKind::Unknown,
                None,
                Some(OS_DEVICE_MULTIPLE_DISKS),
            );
        }
    };

    let kind = match fs::read_to_string(whole.join("queue/rotational")) {
        Ok(value) if value.trim() == "0" => ProbedDeviceKind::Ssd,
        Ok(value) if value.trim() == "1" => ProbedDeviceKind::Hdd,
        _ => ProbedDeviceKind::Unknown,
    };
    let disk_key = read_disk_key(&whole).ok();
    let reason = if kind == ProbedDeviceKind::Unknown || disk_key.is_none() {
        Some(OS_QUERY_FAILED)
    } else {
        None
    };
    probe(kind, disk_key, reason)
}

fn whole_disk(device: &Path) -> Result<PathBuf, ()> {
    match fs::metadata(device.join("partition")) {
        Ok(_) => device.parent().map(Path::to_path_buf).ok_or(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(device.to_path_buf()),
        Err(_) => Err(()),
    }
}

fn slave_entries(device: &Path) -> Result<Vec<PathBuf>, ()> {
    match fs::read_dir(device.join("slaves")) {
        Ok(entries) => entries
            .map(|entry| entry.map(|entry| entry.path()).map_err(|_| ()))
            .collect(),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(_) => Err(()),
    }
}

fn read_disk_key(device: &Path) -> Result<Vec<u8>, ()> {
    let dev = fs::read_to_string(device.join("dev")).map_err(|_| ())?;
    let (major, minor) = dev.trim().split_once(':').ok_or(())?;
    let major = major.parse::<u32>().map_err(|_| ())?;
    let minor = minor.parse::<u32>().map_err(|_| ())?;
    Ok(format!("linux:blk:{major}:{minor}").into_bytes())
}

fn probe(
    kind: ProbedDeviceKind,
    disk_key: Option<Vec<u8>>,
    reason_key: Option<&'static str>,
) -> DeviceProbe {
    DeviceProbe {
        kind,
        disk_key,
        reason_key,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(target_os = "linux")]
    use std::os::unix::fs::MetadataExt;
    use std::{collections::HashMap, error::Error, io};

    struct Fixture {
        _temp: tempfile::TempDir,
        sysfs: PathBuf,
        routes: HashMap<PathBuf, PathBuf>,
    }

    impl Fixture {
        fn new() -> Result<Self, Box<dyn Error>> {
            let temp = tempfile::tempdir()?;
            let sysfs = temp.path().join("sys");
            fs::create_dir_all(sysfs.join("dev/block"))?;
            Ok(Self {
                _temp: temp,
                sysfs,
                routes: HashMap::new(),
            })
        }

        fn device(&self, name: &str, dev: &str, rotational: Option<&str>) -> io::Result<PathBuf> {
            let device = self._temp.path().join("devices").join(name);
            fs::create_dir_all(device.join("slaves"))?;
            fs::write(device.join("dev"), dev)?;
            if let Some(rotational) = rotational {
                fs::create_dir_all(device.join("queue"))?;
                fs::write(device.join("queue/rotational"), rotational)?;
            }
            Ok(device)
        }

        fn partition(&self, name: &str, dev: &str, parent: &Path) -> io::Result<PathBuf> {
            let partition = parent.join(name);
            fs::create_dir_all(&partition)?;
            fs::write(partition.join("partition"), "1")?;
            fs::write(partition.join("dev"), dev)?;
            Ok(partition)
        }

        fn route(&mut self, route: &Path, device: &Path) {
            self.routes
                .insert(route.to_path_buf(), device.to_path_buf());
        }

        fn root_route(&self, major: u32, minor: u32) -> PathBuf {
            self.sysfs
                .join("dev/block")
                .join(format!("{major}:{minor}"))
        }

        fn probe(&self, major: u32, minor: u32) -> DeviceProbe {
            probe_dev_with(&self.sysfs, major, minor, &|path| {
                self.routes
                    .get(path)
                    .cloned()
                    .ok_or_else(|| io::Error::from(io::ErrorKind::NotFound))
            })
        }
    }

    fn expect(
        probe: &DeviceProbe,
        kind: ProbedDeviceKind,
        disk_key: Option<&[u8]>,
        reason: Option<&str>,
    ) -> Result<(), Box<dyn Error>> {
        if probe.kind != kind || probe.disk_key.as_deref() != disk_key || probe.reason_key != reason
        {
            return Err(io::Error::other(format!("unexpected probe: {probe:?}")).into());
        }
        Ok(())
    }

    #[test]
    fn partition_uses_parent_rotational_zero_as_ssd() -> Result<(), Box<dyn Error>> {
        let mut fixture = Fixture::new()?;
        let disk = fixture.device("sda", "8:0", Some("0\n"))?;
        let partition = fixture.partition("sda1", "8:1", &disk)?;
        let route = fixture.root_route(8, 1);
        fixture.route(&route, &partition);
        expect(
            &fixture.probe(8, 1),
            ProbedDeviceKind::Ssd,
            Some(b"linux:blk:8:0"),
            None,
        )
    }

    #[test]
    fn partition_uses_parent_rotational_one_as_hdd() -> Result<(), Box<dyn Error>> {
        let mut fixture = Fixture::new()?;
        let disk = fixture.device("sdb", "8:16", Some("1\n"))?;
        let partition = fixture.partition("sdb1", "8:17", &disk)?;
        fixture.route(&fixture.root_route(8, 17), &partition);
        expect(
            &fixture.probe(8, 17),
            ProbedDeviceKind::Hdd,
            Some(b"linux:blk:8:16"),
            None,
        )
    }

    #[test]
    fn whole_disk_reports_kind_and_key() -> Result<(), Box<dyn Error>> {
        let mut fixture = Fixture::new()?;
        let disk = fixture.device("nvme0n1", "259:0", Some("0"))?;
        fixture.route(&fixture.root_route(259, 0), &disk);
        expect(
            &fixture.probe(259, 0),
            ProbedDeviceKind::Ssd,
            Some(b"linux:blk:259:0"),
            None,
        )
    }

    #[test]
    fn dm_with_two_slaves_reports_multiple_disks() -> Result<(), Box<dyn Error>> {
        let mut fixture = Fixture::new()?;
        let dm = fixture.device("dm-0", "253:0", None)?;
        let first = dm.join("slaves/sda");
        let second = dm.join("slaves/sdb");
        fs::create_dir_all(&first)?;
        fs::create_dir_all(&second)?;
        let disk_a = fixture.device("sda", "8:0", Some("0"))?;
        let disk_b = fixture.device("sdb", "8:16", Some("1"))?;
        fixture.route(&fixture.root_route(253, 0), &dm);
        fixture.route(&first, &disk_a);
        fixture.route(&second, &disk_b);
        expect(
            &fixture.probe(253, 0),
            ProbedDeviceKind::Unknown,
            None,
            Some(OS_DEVICE_MULTIPLE_DISKS),
        )
    }

    #[test]
    fn dm_with_one_slave_follows_the_physical_disk() -> Result<(), Box<dyn Error>> {
        let mut fixture = Fixture::new()?;
        let dm = fixture.device("dm-1", "253:1", None)?;
        let slave = dm.join("slaves/nvme0n1");
        fs::create_dir_all(&slave)?;
        let disk = fixture.device("nvme0n1", "259:0", Some("0"))?;
        fixture.route(&fixture.root_route(253, 1), &dm);
        fixture.route(&slave, &disk);
        expect(
            &fixture.probe(253, 1),
            ProbedDeviceKind::Ssd,
            Some(b"linux:blk:259:0"),
            None,
        )
    }

    #[test]
    fn missing_sysfs_device_is_unknown_without_a_key() -> Result<(), Box<dyn Error>> {
        let fixture = Fixture::new()?;
        expect(&fixture.probe(8, 99), ProbedDeviceKind::Unknown, None, None)
    }

    #[test]
    fn unreadable_rotational_value_reports_query_failure() -> Result<(), Box<dyn Error>> {
        let mut fixture = Fixture::new()?;
        let disk = fixture.device("sdc", "8:32", None)?;
        fs::create_dir_all(disk.join("queue/rotational"))?;
        fixture.route(&fixture.root_route(8, 32), &disk);
        expect(
            &fixture.probe(8, 32),
            ProbedDeviceKind::Unknown,
            Some(b"linux:blk:8:32"),
            Some(OS_QUERY_FAILED),
        )
    }

    #[test]
    fn major_zero_is_unsupported() -> Result<(), Box<dyn Error>> {
        let fixture = Fixture::new()?;
        expect(
            &probe_dev_in(&fixture.sysfs, 0, 42),
            ProbedDeviceKind::Unknown,
            None,
            Some(OS_UNSUPPORTED),
        )
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_open_root_device_smoke() -> Result<(), Box<dyn Error>> {
        let root = fs::File::open("/")?;
        let dev = root.metadata()?.dev();
        let major = ((dev >> 8) & 0x0fff) | ((dev >> 32) & 0xffff_f000);
        let minor = (dev & 0x00ff) | ((dev >> 12) & 0xffff_ff00);
        let result = probe_dev(major as u32, minor as u32);
        eprintln!("Linux open-root device probe: {result:?}");
        if result
            .disk_key
            .as_deref()
            .is_some_and(|key| !key.starts_with(b"linux:blk:"))
        {
            return Err(io::Error::other("Linux disk key has an unexpected prefix").into());
        }
        Ok(())
    }
}
