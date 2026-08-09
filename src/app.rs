use std::io;

use crate::{config::ConfigStore, platform};

pub(crate) fn run() -> io::Result<()> {
    let config_store = ConfigStore::discover()?;
    let _config = config_store.load_or_create()?;

    platform::windows::run()
}
