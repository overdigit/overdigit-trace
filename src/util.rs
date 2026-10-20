use inotify::{EventMask, Inotify, WatchMask};
use std::error::Error;

#[cfg(feature = "placeholders")]
use configparser::ini::Ini;
#[cfg(feature = "placeholders")]
use std::collections::HashMap;
#[cfg(feature = "placeholders")]
use std::sync::OnceLock;

// Credit: https://github.com/nroi/waitforfile/blob/master/src/main.rs
pub fn wait_for(dirname: &std::path::Path) -> Result<bool, Box<dyn Error>> {
    let mut ino = Inotify::init()?;
    let parent = dirname.parent().expect("This folder has no parent. How? It's the default folder, I wrote its path. What did you do?");
    ino.watches()
        .add(parent, WatchMask::DELETE_SELF | WatchMask::CREATE)
        .unwrap();
    if !dirname.exists() {
        loop {
            let mut buffer = [0; 1024];
            let events = ino
                .read_events_blocking(&mut buffer)
                .expect("Error while reading events");
            for event in events {
                match event.name {
                    Some(name) => {
                        if parent.join(name) == dirname {
                            return Ok(true);
                        }
                    }
                    None => {
                        if event.mask == EventMask::DELETE_SELF {
                            return Err("The watched directory has been deleted.".into());
                        }
                    }
                }
            }
        }
    }
    // file already exists prior to running this program.
    Ok(false)
}

pub fn get_uid(username: &str) -> std::io::Result<Option<u32>> {
    Ok(std::fs::read_to_string("/etc/passwd")?
        .lines()
        .find_map(|l| {
            let v = l.split(':').collect::<Vec<_>>();
            if v.len() == 7 && v.first() == Some(&username) {
                return v.get(2)?.parse::<u32>().ok();
            }
            None
        }))
}

#[cfg(feature = "placeholders")]
pub static PLACEHOLDERS: OnceLock<HashMap<String, String>> = OnceLock::new();

#[cfg(feature = "placeholders")]
pub static CODESYS_DEFAULT_CONF: &str = "/etc/codesyscontrol/CODESYSControl.cfg:/etc/codesyscontrol/CODESYSControl_User.cfg:/etc/codesyscontrol/CODESYSControl_WP360.cfg";

#[cfg(feature = "placeholders")]
pub fn read_placeholders(config_paths: &str) -> Result<(), Box<dyn Error>> {
    let mut placeholders: HashMap<String, String> = HashMap::new();
    for config_path in config_paths.split(':') {
        let config = std::fs::read_to_string(config_path)?;
        let mut ini = Ini::new();
        let data = &ini.read(config)?;
        let Some(sysfile) = data.get("sysfile") else {
            continue;
        };
        placeholders.extend(
            sysfile
                .iter()
                .filter(|(k, _v)| {
                    k.strip_prefix("placeholderfilepath.")
                        .and_then(|n| n.parse::<i32>().ok())
                        .is_some()
                })
                .filter_map(|(_k, v)| {
                    v.as_ref()
                        .and_then(|v| v.rsplit_once(','))
                        .map(|(v1, v2)| (v2.to_string(), v1.to_string()))
                }),
        );
    }
    let _ = PLACEHOLDERS.set(placeholders);
    Ok(())
}
