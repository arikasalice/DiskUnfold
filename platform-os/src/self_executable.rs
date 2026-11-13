//! Read access to the running process's own executable image.
use du_core_contracts::product_ports::SelfExecutablePort;
use std::io::{self, Read};

/// Opens the file the operating system reports for the current process image.
///
/// On Linux this is `/proc/self/exe`, which names the running image even after the
/// path was replaced or deleted; other platforms use `std::env::current_exe`.
#[derive(Clone, Copy, Debug, Default)]
pub struct SystemSelfExecutable;

impl SelfExecutablePort for SystemSelfExecutable {
    fn open(&self) -> io::Result<Box<dyn Read + Send>> {
        #[cfg(target_os = "linux")]
        let path = std::path::PathBuf::from("/proc/self/exe");
        #[cfg(not(target_os = "linux"))]
        let path = std::env::current_exe()?;
        Ok(Box::new(std::fs::File::open(path)?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opens_the_running_test_executable() -> io::Result<()> {
        let mut header = [0_u8; 2];
        SystemSelfExecutable.open()?.read_exact(&mut header)?;
        assert_ne!(header, [0, 0]);
        Ok(())
    }
}
