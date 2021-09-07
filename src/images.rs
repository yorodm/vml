use std::borrow::Cow;
use std::cmp::Ordering;
use std::collections::btree_map;
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::fs;
use std::fs::OpenOptions;
use std::io::prelude::*;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use serde::{Deserialize, Serialize};

use crate::config::Images as ConfigImages;
use crate::config_dir;
use crate::files;
use crate::{Error, Result};

#[derive(Clone, Debug)]
pub struct Image<'a> {
    pub description: Option<String>,
    pub name: String,
    pub url: String,
    config: &'a ConfigImages,
    update_after_days: Option<u64>,
}

impl<'a> Image<'a> {
    fn from_deserialize(
        image: DeserializeImage,
        name: impl AsRef<str>,
        config: &'a ConfigImages,
    ) -> Self {
        Image {
            name: name.as_ref().to_string(),
            url: image.url,
            description: image.description,
            config,
            update_after_days: image.update_after_days,
        }
    }

    fn path(&self) -> PathBuf {
        self.config.directory.join(&self.name)
    }

    fn outdate_option(&self) -> Option<bool> {
        let default_update_after_days = self.config.update_after_days;
        let image_path = self.path();
        let modified_time = fs::metadata(image_path).and_then(|m| m.modified()).ok()?;
        let sys_time = SystemTime::now();
        let duration = sys_time.duration_since(modified_time).ok()?;
        let update_after_days = self.update_after_days.or(default_update_after_days);
        let update_after = Duration::from_secs(update_after_days? * 60 * 60 * 24);

        Some(duration > update_after)
    }

    pub fn outdate(&self) -> bool {
        self.outdate_option().unwrap_or(false)
    }

    pub fn exists(&self) -> bool {
        self.path().is_file()
    }

    pub fn pull(&self) -> Result<PathBuf> {
        let mut body =
            reqwest::blocking::get(&self.url).map_err(|e| Error::DownloadImage(e.to_string()))?;
        let image_path = self.path();
        let images_dir = &self.config.directory;
        let mut tmp = tempfile::Builder::new().tempfile_in(images_dir)?;

        println!("Downloading image {} {}", &self.name, &self.url);
        body.copy_to(&mut tmp).map_err(|e| Error::DownloadImage(e.to_string()))?;

        fs::rename(tmp.path(), &image_path)?;

        Ok(image_path)
    }
}

impl PartialEq for Image<'_> {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name
    }
}

impl Eq for Image<'_> {}

impl PartialOrd for Image<'_> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Image<'_> {
    fn cmp(&self, other: &Self) -> Ordering {
        self.name.cmp(&other.name)
    }
}

#[derive(Clone, Debug, Default)]
pub struct Images<'a>(BTreeMap<String, Image<'a>>);

impl Images<'_> {
    pub fn exists(self) -> Self {
        let images = self.0.into_iter().filter(|(_, i)| i.exists()).collect();

        Images(images)
    }

    pub fn outdate(self) -> Self {
        let images = self.0.into_iter().filter(|(_, i)| i.outdate()).collect();

        Images(images)
    }

    pub fn filter(self, predicate: impl Fn(&Image) -> bool) -> Self {
        let images = self.0.into_iter().filter(|(_, i)| predicate(i)).collect();

        Images(images)
    }

    pub fn names(&self) -> BTreeSet<String> {
        self.0.iter().map(|(name, _)| name.to_string()).collect()
    }

    pub fn get<'a>(&'a self, name: impl AsRef<str>) -> Option<&'a Image> {
        self.0.get(name.as_ref())
    }

    pub fn get_result<'a>(&'a self, name: impl AsRef<str>) -> Result<&'a Image> {
        self.0.get(name.as_ref()).ok_or_else(|| Error::UnknownImage(name.as_ref().to_string()))
    }
}

impl<'a> IntoIterator for Images<'a> {
    type Item = Image<'a>;
    type IntoIter = ImagesIntoIter<'a>;

    fn into_iter(self) -> Self::IntoIter {
        ImagesIntoIter(self.0.into_iter())
    }
}

pub struct ImagesIntoIter<'a>(btree_map::IntoIter<String, Image<'a>>);

impl<'a> Iterator for ImagesIntoIter<'a> {
    type Item = Image<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        self.0.next().map(|(_, i)| i)
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
#[serde(deny_unknown_fields)]
struct DeserializeImage {
    pub description: Option<String>,
    pub url: String,
    #[serde(default)]
    change: Vec<String>,
    update_after_days: Option<u64>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
#[serde(transparent)]
struct DeserializeImages(BTreeMap<String, DeserializeImage>);

fn update_images(
    embedded_images: &mut btree_map::IntoIter<String, DeserializeImage>,
    config_images: &mut btree_map::IntoIter<String, DeserializeImage>,
) -> BTreeMap<String, DeserializeImage> {
    let mut embedded_image = embedded_images.next();
    let mut config_image = config_images.next();
    let mut images: BTreeMap<String, DeserializeImage> = BTreeMap::new();

    while let (Some(ei), Some(ci)) = (&embedded_image, &config_image) {
        let old_name = &ci.0;
        let new_name = &ei.0;
        let old = &ci.1;
        let new = &ei.1;
        let change_set: HashSet<&str> = old.change.iter().map(AsRef::as_ref).collect();
        match new_name.cmp(old_name) {
            Ordering::Greater => {
                if !change_set.contains("delete") {
                    images.insert(old_name.to_owned(), old.to_owned());
                };
                config_image = config_images.next();
            }
            Ordering::Less => {
                images.insert(new_name.to_owned(), new.to_owned());
                embedded_image = embedded_images.next();
            }
            Ordering::Equal => {
                let update_all = change_set.contains("update-all");
                let url = if change_set.contains("keep-url")
                    || !update_all && !change_set.contains("update-url")
                {
                    old.url.to_owned()
                } else {
                    new.url.to_owned()
                };
                let description = if change_set.contains("keep-description")
                    || !update_all && !change_set.contains("update-description")
                {
                    old.description.to_owned()
                } else {
                    new.description.to_owned()
                };
                let change = if change_set.contains("keep-change")
                    || !update_all && !change_set.contains("update-change")
                {
                    old.change.to_owned()
                } else {
                    new.change.to_owned()
                };
                let update_after_days = if change_set.contains("keep-update-after-days")
                    || !update_all && !change_set.contains("update-update-after-days")
                {
                    old.update_after_days.to_owned()
                } else {
                    new.update_after_days.to_owned()
                };
                images.insert(
                    old_name.to_owned(),
                    DeserializeImage { url, description, change, update_after_days },
                );
                embedded_image = embedded_images.next();
                config_image = config_images.next();
            }
        }
    }

    images
}

pub fn update_images_file(embedded_iamges_toml: Cow<'static, [u8]>) -> Result<()> {
    let mut embedded_images = toml::from_slice::<DeserializeImages>(&embedded_iamges_toml)
        .expect("Bad embedded images.toml")
        .0
        .into_iter();
    let images_str = &fs::read_to_string(images_file_path())?;
    let mut config_images = toml::from_str::<DeserializeImages>(images_str)
        .map_err(|e| {
            Error::parse_images_file(&images_file_path().to_string_lossy(), &e.to_string())
        })?
        .0
        .into_iter();

    let images = update_images(&mut embedded_images, &mut config_images);

    let mut images_file = OpenOptions::new().truncate(true).write(true).open(images_file_path())?;
    let header = files::get_file("images-header")?;
    images_file.write_all(&header)?;
    let images_string = toml::to_string(&images).expect("Bad internal images representation");
    images_file.write_all(images_string.as_bytes())?;

    Ok(())
}

fn images_file_path() -> PathBuf {
    config_dir().join("images.toml")
}

fn parse(images_file_path: &Path) -> Result<DeserializeImages> {
    let images_str = &fs::read_to_string(images_file_path)?;
    let images = toml::from_str(images_str).map_err(|e| {
        Error::parse_images_file(&images_file_path.to_string_lossy(), &e.to_string())
    })?;

    Ok(images)
}

pub fn path(images_dir: &Path, image_name: &str) -> Result<PathBuf> {
    let image_path = images_dir.join(image_name);
    if image_path.is_file() {
        Ok(image_path)
    } else {
        Err(Error::ImageDoesNotExists(image_name.to_string()))
    }
}

pub fn find(images_dirs: &[&PathBuf], image_name: &str) -> Result<PathBuf> {
    for images_dir in images_dirs {
        let image_path = images_dir.join(image_name);
        if image_path.is_file() {
            return Ok(image_path);
        }
    }

    Err(Error::ImageDoesNotExists(image_name.to_string()))
}

pub fn list(images_dirs: &[&PathBuf]) -> Result<Vec<String>> {
    let mut images = BTreeSet::new();

    for dir in images_dirs {
        for path in fs::read_dir(dir)? {
            let name = path.unwrap().file_name().to_string_lossy().to_string();
            images.insert(name);
        }
    }

    Ok(images.into_iter().collect())
}

pub fn available(config: &ConfigImages) -> Result<Images> {
    let images = parse(&images_file_path())?.0;
    let images = images
        .into_iter()
        .map(|(k, v)| (k.to_owned(), Image::from_deserialize(v, &k, config)))
        .collect();

    Ok(Images(images))
}

pub fn remove(images_dir: &Path, image_name: &str) -> Result<()> {
    let image_path = images_dir.join(image_name);
    fs::remove_file(&image_path)?;
    Ok(())
}
