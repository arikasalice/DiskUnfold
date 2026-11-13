//! Read access to the running process's own executable image.
use du_core_contracts::product_ports::SelfExecutablePort;
use std::io::{self, Read};

/// Opens the file the operating system reports for the current process image.
#[derive(Clone, Copy, Debug, Default)]
pub struct SystemSelfExecutable;

impl SelfExecutablePort for SystemSelfExecutable {
    fn open(&self) -> io::Result<Box<dyn Read + Send>> {
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
