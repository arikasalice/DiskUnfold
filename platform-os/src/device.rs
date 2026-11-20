//! Device identity observations for an already-open volume.
use std::fmt;

#[cfg(any(target_os = "linux", test))]
pub mod linux;
#[cfg(target_os = "macos")]
#[allow(unsafe_code)]
pub mod macos;
#[cfg(target_os = "windows")]
#[allow(unsafe_code)]
pub mod windows;

pub const OS_QUERY_FAILED: &str = "os.query_failed";
pub const OS_UNSUPPORTED: &str = "os.unsupported";
pub const OS_DEVICE_NETWORK: &str = "os.device_network";
pub const OS_DEVICE_MULTIPLE_DISKS: &str = "os.device_multiple_disks";

/// The storage device kind reported by the operating system.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProbedDeviceKind {
    Ssd,
    Hdd,
    Unknown,
}

/// Device observations for an already-open volume.
#[derive(Clone, Eq, PartialEq)]
pub struct DeviceProbe {
    pub kind: ProbedDeviceKind,
    pub disk_key: Option<Vec<u8>>,
    pub reason_key: Option<&'static str>,
}

impl fmt::Debug for DeviceProbe {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        struct DiskKey(bool);

        impl fmt::Debug for DiskKey {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                if self.0 {
                    formatter.write_str("Some(<redacted>)")
                } else {
                    formatter.write_str("None")
                }
            }
        }

        formatter
            .debug_struct("DeviceProbe")
            .field("kind", &self.kind)
            .field("disk_key", &DiskKey(self.disk_key.is_some()))
            .field("reason_key", &self.reason_key)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io;

    #[test]
    fn debug_redacts_disk_key_bytes() -> Result<(), Box<dyn std::error::Error>> {
        let probe = DeviceProbe {
            kind: ProbedDeviceKind::Ssd,
            disk_key: Some(b"private-disk-id".to_vec()),
            reason_key: None,
        };
        let formatted = format!("{probe:?}");
        if formatted != "DeviceProbe { kind: Ssd, disk_key: Some(<redacted>), reason_key: None }" {
            return Err(io::Error::other("device key was not redacted in Debug output").into());
        }

        let absent = DeviceProbe {
            kind: ProbedDeviceKind::Unknown,
            disk_key: None,
            reason_key: Some(OS_QUERY_FAILED),
        };
        if !format!("{absent:?}").contains("disk_key: None") {
            return Err(io::Error::other("absent device key was not formatted as None").into());
        }
        Ok(())
    }
}
