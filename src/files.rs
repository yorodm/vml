use std::borrow::Cow;
use std::fs;
use std::path::Path;

use cmd_lib::run_cmd;
use rust_embed::RustEmbed;

use crate::config::Config;
use crate::config_dir;
use crate::{Error, Result};

#[derive(RustEmbed)]
#[folder = "files/configs"]
struct AssetConfigs;

#[derive(RustEmbed)]
#[folder = "files/get-url-progs"]
struct AssetGetUrlProgs;

pub fn get_config<S: AsRef<str>>(path: S) -> Result<Cow<'static, [u8]>> {
    AssetConfigs::get(path.as_ref())
        .map(|f| f.data)
        .ok_or_else(|| Error::GetWrongEmbeddedFile(path.as_ref().to_string()))
}

fn install_get_url_progs() -> Result<()> {
    let directory = config_dir().join("get-url-progs");
    fs::create_dir_all(&directory)?;

    for filename in AssetGetUrlProgs::iter() {
        let filename = filename.as_ref();
        let filepath = directory.join(filename);
        let content = AssetGetUrlProgs::get(filename).unwrap();
        fs::write(&filepath, content.data)?;
        run_cmd!(chmod +x $filepath)?
    }

    Ok(())
}

fn install_config(filename: &str) -> Result<()> {
    let directory = config_dir();
    fs::create_dir_all(&directory)?;

    let config = &directory.join(filename);
    if !config.exists() {
        let etc_config = Path::new("/etc/vml").join(filename);
        if etc_config.exists() {
            fs::copy(etc_config, config)?;
        } else {
            let content = AssetConfigs::get(filename).unwrap();
            fs::write(&config, content.data)?;
        }
    }

    Ok(())
}

pub fn install_main_config() -> Result<()> {
    install_config("config.toml")?;

    Ok(())
}

pub fn install_all(config: &Config) -> Result<()> {
    if !config.vms_dir.exists() {
        fs::create_dir_all(&config.vms_dir)?;
    }
    if !config.images.directory.exists() {
        fs::create_dir_all(&config.images.directory)?;
    }
    install_config("images.toml")?;

    install_get_url_progs()?;

    Ok(())
}
