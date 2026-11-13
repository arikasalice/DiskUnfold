//! Prints the startup Full Disk Access observation for manual OS evidence.

use du_platform_os::startup::{StartupCapabilitiesPort, SystemStartupCapabilities};

fn main() {
    println!("{:?}", SystemStartupCapabilities.full_disk_access_status());
}
