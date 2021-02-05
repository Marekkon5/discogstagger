extern crate metaflac;
extern crate strsim;
extern crate regex;

use walkdir::WalkDir;
use regex::Regex;
use strsim::normalized_levenshtein;
use id3::{Tag, Version, Timestamp};
use chrono::{NaiveDate, Datelike};
use metaflac::block::PictureType as FLACPictureType;
use id3::frame::PictureType as ID3PictureType;
use id3::frame::Picture;
use std::fs::File;
use std::io::prelude::*;
use std::io::SeekFrom;

use crate::discogs::{Discogs, Track, ReleaseMaster, ReleaseType};
use crate::ui;

#[derive(Debug, Clone)]
pub enum MusicFileType {
    AIFF,
    MP3,
    FLAC
}

#[derive(Debug, Clone)]
pub struct MusicFileInfo {
    pub path: String,
    pub title: String,
    pub artists: Vec<String>,
    pub tag: MusicFileType 
}

#[derive(Debug, Clone)]
pub struct TaggerConfig {
    //Tags
    pub title: bool,
    pub artist: bool,
    pub album: bool,
    pub label: bool,
    pub date: bool,
    pub track: bool,
    pub art: bool,

    //Genres
    // 0 No Style/Genre
    // 1 Only Style
    // 2 Only Genre
    // 3 Merge Genre + Style
    pub id3_genre: i8,

    // 0 No Style/Genre
    // 1 Both
    // 2 Only Style (in Genre tag)
    // 3 Only Genre
    // 4 Merge Genre + Style
    pub flac_genre: i8,

    //Other
    pub artist_separator: String,
    pub fuzziness: u8,
    pub overwrite: bool,
    pub id3v23: bool
}

pub fn match_track(discogs: &mut Discogs, info: &MusicFileInfo, fuzziness: u8) -> Result<Option<(Track, ReleaseMaster)>, Box<dyn std::error::Error>> {
    //Search
    let mut results = discogs.search(Some("release,master"), Some(&format!("{} {}", clean_title(&info.title, false), &info.artists.first().unwrap())), None, None)?;
    //Fallback
    if (&results).is_none() || results.as_ref().unwrap().releases.is_empty() {
        results = discogs.search(Some("release,master"), None, Some(&info.title), Some(info.artists.first().unwrap()))?;
    }
    //No results
    if results.is_none() {
        return Ok(None)
    }
    //Match release & track
    let mut r = results.unwrap();
    r.releases.truncate(2);
    r.masters.truncate(2);
    for release_data in vec![r.masters, r.releases].concat() {
        //Get full release
        let release = match release_data.rtype {
            ReleaseType::Release => discogs.release(release_data.id)?,
            ReleaseType::Master => discogs.master(release_data.id, release_data.label)?
        };
        if (&release).is_none() || release.as_ref().unwrap().tracks.is_none() {
            continue;
        }
        //Match track
        let mut fuzzy_tracks: Vec<(u8, Track)> = vec![];
        for t in release.as_ref().unwrap().tracks.as_ref().unwrap() {
            //Exact match = return
            if clean_title(&t.title, true) == clean_title(&info.title, true) {
                return Ok(Some((t.to_owned(), release.unwrap())));
            }
            //Fuzzy
            fuzzy_tracks.push(((normalized_levenshtein(&clean_title(&t.title, true), &clean_title(&info.title, true)) * 100_f64) as u8, t.clone()));
        }
        let mut fuzzy_tracks: Vec<(u8, Track)> = release.as_ref().unwrap().tracks.as_ref().unwrap().iter().map(|t| {
            ((normalized_levenshtein(&clean_title(&t.title, true), &clean_title(&info.title, true)) * 100_f64) as u8, t.clone())
        }).collect();
        //Sort fuzzy results
        fuzzy_tracks.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap());
        if fuzzy_tracks.first().unwrap().0 >= fuzziness {
            return Ok(Some((fuzzy_tracks.first().unwrap().1.clone(), release.unwrap())));
        }
    }
    Ok(None)
}

//Matching = if matching remove also symbols
fn clean_title(title: &str, matching: bool) -> String {
    let lowercase = title.to_lowercase();
    //Remove feat
    let mut re = Regex::new(r"(\(*)feat(\.*).*(\)*)$").unwrap();
    let result = re.replace(&lowercase, "");
    //Remove original mix
    re = Regex::new(r"(\(*)original( (mix|version))*(\)*)$").unwrap();
    let out = re.replace(&result, "");
    //Remove space, some problematic characters
    if matching {
        return out.to_string().replace(" ", "").replace(";", "").replace("&", "");
    }
    out.to_string()
}

pub fn get_files(path: &str) -> Vec<MusicFileInfo> {
    //Supported extensions
    let supported_extensions = vec![".mp3", ".flac", ".aif", ".aiff"];
    //List of filenames of supported formats for path
    WalkDir::new(path).into_iter().filter(
        |e| e.is_ok() && 
        supported_extensions.iter().any(|&i| e.as_ref().unwrap().path().to_str().unwrap().to_ascii_lowercase().ends_with(i))
    ).map(|e| e.unwrap().path().to_str().unwrap().to_owned())
    //Load info
    .filter_map(|f| {
        //Debug
        #[cfg(debug_assertions)]
        println!("Loading track: {}", f);

        match load_file_info(&f) {
            Ok(i) => Some(i),
            Err(_) => {
                ui::print_warning(&format!("Invalid track: {}", path));
                None
            }
        }
    }).collect()
}

//Wrapper to load by format
pub fn load_file_info(path: &str) -> Result<MusicFileInfo, Box<dyn std::error::Error>> {
    if path.to_ascii_lowercase().ends_with(".flac") {
        return load_flac_info(path);
    }
    load_id3_info(path)   
}

//Load ID3 metadata from MP3
fn load_id3_info(path: &str) -> Result<MusicFileInfo, Box<dyn std::error::Error>> {
    let mut tag_type = MusicFileType::MP3;
    let tag = if path.ends_with(".aif") || path.ends_with(".aiff") {
        tag_type = MusicFileType::AIFF;
        Tag::read_from_aiff(path)?
    } else {
        Tag::read_from_path(path)?
    };

    Ok(MusicFileInfo {
        path: path.to_owned(),
        title: tag.title().ok_or("Missing title tag!")?.to_owned(),
        artists: parse_artist_tag(tag.artist().ok_or("Missing artist tag!")?),
        tag: tag_type
    })
}

//Load FLAC meta
fn load_flac_info(path: &str) -> Result<MusicFileInfo, Box<dyn std::error::Error>> {
    //Load header
    let mut file = File::open(path)?;
    let mut header: [u8; 4] = [0; 4];
    file.read_exact(&mut header)?;
    //Check for FLAC with ID3
    if &header[0..3] == b"ID3" {
        ui::print_warning(&format!("FLAC with ID3 tags are not supported, and should not be used. Consider converting this track metadata to Vorbis! {}", path));
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "FLAC ID3 not supported!").into());
    }
    //Check if FLAC
    if &header != b"fLaC" {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "Not a valid FLAC!").into());
    }
    file.seek(SeekFrom::Start(0))?;
    //Load tag
    let tag = metaflac::Tag::read_from(&mut file)?;
    let vorbis = tag.vorbis_comments().ok_or("Missing Vorbis Comments!")?;
    //Parse artists
    let artists = match vorbis.artist().ok_or("Missing artists!")?.len() {
        //No artists
        0 => None.ok_or("Missing artists!")?,
        //Single artist tag - manually parse
        1 => parse_artist_tag(vorbis.artist().unwrap().first().unwrap()),
        //Multiple artist tags = don't parse
        _ => vorbis.artist().unwrap().to_owned()
    };

    Ok(MusicFileInfo {
        path: path.to_owned(),
        title: vorbis.title().ok_or("Missing title!")?.first().ok_or("Missing title!")?.to_owned(),
        artists,
        tag: MusicFileType::FLAC
    })
}

//Wrapper to write tags by format
pub fn write_tag(discogs: &mut Discogs, config: &TaggerConfig, info: &MusicFileInfo, release: &ReleaseMaster, track: &Track) -> Result<(), Box<dyn std::error::Error>> {
    //Get tag by type
    let mut tag = match info.tag {
        MusicFileType::FLAC => return write_flac_tag(discogs, config, &info.path, release, track),
        MusicFileType::MP3 => {
            Tag::read_from_path(&info.path)?
        },
        MusicFileType::AIFF => {
            Tag::read_from_aiff(&info.path)?
        }
    };
    //Write
    write_id3_tag(&mut tag, discogs, config, release, track)?;
    let version = match config.id3v23 {
        true => Version::Id3v23,
        false => Version::Id3v24
    };
    //Save
    match info.tag {
        MusicFileType::MP3 => tag.write_to_path(&info.path, version)?,
        MusicFileType::AIFF => tag.write_to_aiff(&info.path, version)?,
        //Shouldn't happen
        MusicFileType::FLAC => {}
    };

    Ok(())
}

fn write_flac_tag(discogs: &mut Discogs, config: &TaggerConfig, path: &str, release: &ReleaseMaster, track: &Track) -> Result<(), Box<dyn std::error::Error>> {
    let mut tag = metaflac::Tag::read_from_path(path)?;
    let vorbis = tag.vorbis_comments_mut();

    //Tags
    if config.title && config.overwrite {
        vorbis.set_title(vec![track.title.to_owned()]);
    }
    if config.album && (config.overwrite || vorbis.album().is_none()) {
        vorbis.set_album(vec![release.title.to_owned()]);
    }
    if config.artist && config.overwrite {
        vorbis.set_artist(track.artists.as_ref().unwrap_or_else(|| release.artists.as_ref().unwrap())
            .iter().map(|a| clean_discogs_artist(a)).collect::<Vec<String>>());
    }
    if config.label && release.label.is_some() && !release.label.as_ref().unwrap().is_empty() && (config.overwrite || vorbis.get("LABEL").is_none()) {
        vorbis.set("LABEL", vec![clean_discogs_artist(release.label.as_ref().unwrap().first().unwrap())]);
    }
    if config.date && (config.overwrite || vorbis.get("DATE").is_none()) {
        if release.released.is_some() {
            vorbis.set("DATE", vec![release.released.to_owned().unwrap()]);
        } else {
            if release.year.is_some() {
                vorbis.set("DATE", vec![release.year.unwrap().to_string()]);
            }
        }
    }
    if config.flac_genre > 0 {
        //Sort
        let mut genres = release.genres.clone();
        let mut styles = release.styles.clone();
        genres.sort();
        styles.sort();
        //Write
        //Genre
        if (config.flac_genre == 1 || config.flac_genre == 3) && (config.overwrite || vorbis.get("GENRE").is_none()) {
            vorbis.set("GENRE", genres.clone());
        }
        //Styles in genre
        if config.flac_genre == 2 && (config.overwrite || vorbis.get("GENRE").is_none()) {
            vorbis.set("GENRE", styles.clone());
        }
        //Style
        if config.flac_genre == 1 && (config.overwrite || vorbis.get("STYLE").is_none()) {
            vorbis.set("STYLE", styles.clone());
        }
        //Both
        if config.flac_genre == 4 && (config.overwrite || vorbis.get("GENRE").is_none()) {
            genres.append(&mut styles);
            genres.sort();
            vorbis.set("GENRE", genres);
        }
    }

    if config.track && (config.overwrite || vorbis.track().is_none()) {
        vorbis.set_track(track.position_int as u32);
    }
    //Art
    if config.art && release.art_url.is_some() && (config.overwrite || tag.pictures().count() == 0) {
        match discogs.download_art(release.art_url.as_ref().unwrap()) {
            Ok(data) => {
                tag.remove_picture_type(FLACPictureType::CoverFront);
                tag.add_picture("image/jpeg", FLACPictureType::CoverFront, data);
            },
            Err(_) => eprintln!("Error downloading album art, ignoring!")
        }
    }

    //Save
    tag.save()?;
    Ok(())
}

fn write_id3_tag(tag: &mut Tag, discogs: &mut Discogs, config: &TaggerConfig, release: &ReleaseMaster, track: &Track) -> Result<(), Box<dyn std::error::Error>> {
    //Tags
    if config.title && config.overwrite {
        tag.set_title(&track.title);
    }
    if config.album && (config.overwrite || tag.album().is_none()) {
        tag.set_album(&release.title);
    }
    if config.artist && config.overwrite {
        tag.set_artist(track.artists.as_ref().unwrap_or_else(|| release.artists.as_ref().unwrap())
            .iter().map(|a| clean_discogs_artist(a)).collect::<Vec<String>>().join(&config.artist_separator));
    }
    if config.label && release.label.is_some() && !release.label.as_ref().unwrap().is_empty() && (config.overwrite || tag.get("TPUB").is_none()) {
        tag.set_text("TPUB", clean_discogs_artist(release.label.as_ref().unwrap().first().unwrap()));
    }
    if config.date && release.year.is_some() {
        //Parse date
        let mut month = None;
        let mut day = None;
        if release.released.is_some() && release.released.as_ref().unwrap().len() == 10 {
            if let Ok(d) = NaiveDate::parse_from_str(release.released.as_ref().unwrap(), "%Y-%m-%d") {
                month = Some(d.month() as u8);
                day = Some(d.day() as u8);
            }
        }
        //ID3v2.4
        if !config.id3v23 && (config.overwrite || tag.date_recorded().is_none()) {
            //Remove ID3v2.3
            tag.remove("TDAT");
            tag.remove("TYER");

            tag.set_date_recorded(Timestamp {
                year: release.year.unwrap() as i32,
                month,
                day,
                hour: None,
                minute: None,
                second: None
            });
        }

        //ID3v2.3
        if config.id3v23 {
            //Date
            if (config.overwrite || tag.get("TDAT").is_none()) && (month.is_some() && day.is_some()) {
                tag.remove_date_recorded();
                tag.set_text("TDAT", &format!("{:02}{:02}", day.unwrap(), month.unwrap()));
            }
            //Year
            if config.overwrite || tag.get("TYER").is_none() {
                tag.remove_date_recorded();
                tag.set_text("TYER", release.year.unwrap().to_string());
            }
        }

    }
    if config.id3_genre > 0 && (config.overwrite || tag.genre().is_none()) {
        match config.id3_genre {
            //Only style
            1 => {
                let mut styles = release.styles.clone();
                styles.sort();
                tag.set_genre(styles.join(", "));
            },
            //Only genre
            2 => {
                let mut genres = release.genres.clone();
                genres.sort();
                tag.set_genre(genres.join(", "));
            },
            //Merge 
            3 => {
                let mut styles = release.styles.clone();
                styles.append(&mut release.genres.clone());
                styles.sort();
                tag.set_genre(styles.join(", "));
            },
            _ => {}
        }
    }
    if config.track && (config.overwrite || tag.track().is_none()) {
        tag.set_track(track.position_int as u32);
    }
    //Art
    if config.art && release.art_url.is_some() && (config.overwrite || tag.pictures().count() == 0) {
        match discogs.download_art(release.art_url.as_ref().unwrap()) {
            Ok(data) => {
                tag.remove_picture_by_type(ID3PictureType::CoverFront);
                tag.add_picture(Picture {
                    mime_type: "image/jpeg".to_string(),
                    picture_type: ID3PictureType::CoverFront,
                    description: "Cover".to_string(),
                    data
                });
            },
            Err(_) => eprintln!("Error downloading album art, ignoring!")
        }
    }

    Ok(())
}

fn clean_discogs_artist(name: &str) -> String {
    let re = Regex::new(r" \(\d{1,2}\)$").unwrap();
    re.replace(name, "").to_string()
}

//Try to split artist string with common separators
fn parse_artist_tag(src: &str) -> Vec<String> {
    if src.contains(';') {
        return src.split(';').collect::<Vec<&str>>().into_iter().map(|v| v.to_owned()).collect();
    }
    if src.contains(',') {
        return src.split(',').collect::<Vec<&str>>().into_iter().map(|v| v.to_owned()).collect();
    }
    if src.contains('/') {
        return src.split('/').collect::<Vec<&str>>().into_iter().map(|v| v.to_owned()).collect();
    }
    vec![src.to_owned()]
}
