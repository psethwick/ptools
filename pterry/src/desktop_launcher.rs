use crate::picker::{Executable, Item, ListItem, Picker};
use log::debug;
use nom::{
    branch::alt,
    bytes::complete::{tag, take_till, take_until},
    combinator::rest,
    error::Error,
    sequence::preceded,
    Finish, IResult,
};
use std::{env, str::FromStr};
use std::{fs, path::PathBuf};

#[derive(Debug, PartialEq, Eq)]
pub struct DesktopEntry {
    pub name: String,
    pub exec: String,
    pub args: Vec<String>,
}

impl DesktopEntry {
    fn new(name: String, exec: String, args: Vec<String>) -> Self {
        Self { name, exec, args }
    }
}

impl FromStr for DesktopEntry {
    type Err = Error<String>;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match parse_desktop_entry(s).finish() {
            Ok((_remaining, de)) => Ok(de),
            Err(Error { input, code }) => Err(Error {
                input: input.to_string(),
                code,
            }),
        }
    }
}

pub struct DesktopLauncher {
    apps: Vec<ListItem>,
}

impl Default for DesktopLauncher {
    fn default() -> Self {
        // TODO: platform specific
        let applications_folders = match env::var("XDG_DATA_DIRS") {
            Ok(dd) => dd
                .split(':')
                .map(|p| PathBuf::from(format!("{}/applications", p)))
                .collect(),
            Err(_) => vec![
                // let's just try our best
                PathBuf::from("/usr/share/applications"),
                PathBuf::from("/usr/local/share/applications"),
                PathBuf::from("$HOME/.local/share/applications"),
            ],
        };
        let mut apps = Vec::<ListItem>::with_capacity(512);
        for applications_folder in applications_folders {
            let dir = applications_folder.read_dir();
            if dir.is_err() {
                continue;
            }
            for entry in dir.unwrap().flatten() {
                let path = entry.path();
                if let Some(ost) = path.extension() {
                    if let Some(ext) = ost.to_str() {
                        if ext == "desktop" {
                            match fs::read_to_string(path) {
                                Ok(contents) => match contents.parse::<DesktopEntry>() {
                                    Ok(de) => {
                                        apps.push(ListItem {
                                            highlights: None,
                                            display: de.name,
                                            item: Item::Executable(Executable {
                                                exec: de.exec,
                                                args: de.args,
                                            }),
                                        });
                                    }
                                    Err(e) => {
                                        debug!("{:?}, {:?}", e, entry.path().to_str());
                                    }
                                },
                                Err(err) => {
                                    debug!("{err:?}");
                                }
                            }
                        }
                    }
                }
            }
        }
        Self { apps }
    }
}

impl Picker for DesktopLauncher {
    fn name(&self) -> &str {
        ".desktop file launcher"
    }

    fn cacheable(&self) -> Option<Vec<ListItem>> {
        Some(self.apps.clone())
    }
}

fn parse_desktop_entry_group(i: &str) -> IResult<&str, &str> {
    preceded(
        preceded(take_until("[Desktop Entry]"), tag("[Desktop Entry]\n")),
        alt((take_until("\n["), rest)),
    )(i)
}

fn parse_name(i: &str) -> IResult<&str, &str> {
    preceded(
        preceded(take_until("Name="), tag("Name=")),
        take_till(|c| c == '\n'),
    )(i)
}

fn parse_exec(i: &str) -> IResult<&str, &str> {
    preceded(
        // newline is to avoid matching TryExec
        preceded(take_until("\nExec="), tag("\nExec=")),
        take_till(|c| c == '\n'),
    )(i)
}
// now that I've built it, I wonder if I should do generic key/value store instead
// maybe later
fn parse_desktop_entry(i: &str) -> IResult<&str, DesktopEntry> {
    let (_, group) = parse_desktop_entry_group(i)?;
    let (_, name) = parse_name(group)?;
    let (_, exec) = parse_exec(group)?;
    let mut actual_exec = exec;
    let args: Vec<String> = exec
        .split_whitespace()
        .enumerate()
        .filter_map(|(i, word)| {
            if i == 0 {
                actual_exec = word;
                None
            } else {
                Some(word.to_string())
            }
        })
        .collect();

    Ok((
        "",
        DesktopEntry::new(name.to_string(), actual_exec.to_string(), args),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    const DESKTOP_ENTRY: &str = "[Desktop Entry]
Type=Application
TryExec=alacritty
Exec=alacritty hey
Icon=Alacritty
Terminal=false
Categories=System;TerminalEmulator;

Name=Alacritty
GenericName=Terminal
Comment=A fast, cross-platform, OpenGL terminal emulator
StartupNotify=true
StartupWMClass=Alacritty
Actions=New;
X-Desktop-File-Install-Version=0.27

[Desktop Action New]
Name=New Terminal
Exec=alacritty";
    #[test]
    fn test_parse_desktop_entry_group() {
        assert_eq!(
            parse_desktop_entry_group(
                "[Desktop Entry]
Type=Application
TryExec=alacritty
Exec=alacritty
Icon=Alacritty
Terminal=false
Categories=System;TerminalEmulator;

Name=Alacritty
GenericName=Terminal
Comment=A fast, cross-platform, OpenGL terminal emulator
StartupNotify=true
StartupWMClass=Alacritty
Actions=New;
X-Desktop-File-Install-Version=0.27

[Desktop Action New]
Name=New Terminal
Exec=alacritty"
            ),
            Ok((
                "\n[Desktop Action New]\nName=New Terminal\nExec=alacritty",
                "Type=Application
TryExec=alacritty
Exec=alacritty
Icon=Alacritty
Terminal=false
Categories=System;TerminalEmulator;

Name=Alacritty
GenericName=Terminal
Comment=A fast, cross-platform, OpenGL terminal emulator
StartupNotify=true
StartupWMClass=Alacritty
Actions=New;
X-Desktop-File-Install-Version=0.27\n"
            ))
        );
    }
    #[test]
    fn test_parse_name() {
        assert_eq!(parse_name("Name=Test\n"), Ok(("\n", "Test")));
        assert_eq!(parse_name("Name=Test"), Ok(("", "Test")));
        assert_eq!(parse_name("Name=Test Test"), Ok(("", "Test Test")));
    }

    #[test]
    fn test_parse_exec() {
        assert_eq!(parse_exec("\nExec=alacritty\n"), Ok(("\n", "alacritty")))
    }

    #[test]
    fn test_parse_desktop_entry() {
        assert_eq!(
            parse_desktop_entry(DESKTOP_ENTRY),
            Ok((
                "",
                DesktopEntry::new(
                    "Alacritty".to_string(),
                    "alacritty".to_string(),
                    vec!["hey".to_string()]
                )
            ))
        );
    }
}
