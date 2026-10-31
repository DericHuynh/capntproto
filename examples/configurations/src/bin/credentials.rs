use capntproto_configuration_example::{Pki, Result};
fn main() -> Result<()> {
    let path = std::env::args_os()
        .nth(1)
        .ok_or("usage: example-credentials DIRECTORY (must be empty)")?;
    Pki::generate()?.write(std::path::Path::new(&path))?;
    println!("Development CA and seven-day P-256 certificates written.");
    Ok(())
}
