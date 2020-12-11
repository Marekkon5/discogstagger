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

use crate::discogs::{Discogs, Track, ReleaseMaster, ReleaseType};

#[derive(Debug, Clone)]
pub struct MusicFileInfo {
    pub path: String,
    pub title: String,
    pub artists: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct TaggerConfig {
    //Tags
    pub title: bool,
    pub artist: bool,
    pub album: bool,
    pub label: bool,
    pub genre: bool,
    pub date: bool,
    pub track: bool,
    pub art: bool,

    //Other
    pub artist_separator: String,
    pub fuzziness: u8,
    pub use_styles: bool
}

pub fn match_track(discogs: &mut Discogs, info: &MusicFileInfo, fuzziness: u8) -> Result<Option<(Track, ReleaseMaster)>, Box<dyn std::error::Error>> {
    //Search
    let mut results = discogs.search(Some("release,master"), Some(&format!("{} {}", clean_title(&info.title, false), &info.artists.first().unwrap())), None, None)?;
    //Fallback
    if (&results).is_none() || &results.as_ref().unwrap().releases.len() == &0 {
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
        let mut fuzzy_tracks: Vec<(u8, Track)> = release.as_ref().unwrap().tracks.as_ref().unwrap().into_iter().map(|t| {
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

//Simple one-liner
pub fn get_files(path: &str) -> Vec<MusicFileInfo> {
    //List of filenames of supported formats for path
    WalkDir::new(path).into_iter().filter(
        |e| e.is_ok() && 
        e.as_ref().unwrap().path().to_str().unwrap().to_ascii_lowercase().ends_with(".mp3") ||
        e.as_ref().unwrap().path().to_str().unwrap().to_ascii_lowercase().ends_with(".flac")
    ).map(|e| e.unwrap().path().to_str().unwrap().to_owned())
    //Load info
    .filter_map(|f| {
        match load_file_info(&f) {
            Ok(i) => Some(i),
            Err(_) => None
        }
    }).collect()
}

//Wrapper to load by format
pub fn load_file_info(path: &str) -> Result<MusicFileInfo, Box<dyn std::error::Error>> {
    if path.to_ascii_lowercase().ends_with(".mp3") {
        return load_mp3_info(path);
    }
    return load_flac_info(path);
}

//Load ID3 metadata from MP3
fn load_mp3_info(path: &str) -> Result<MusicFileInfo, Box<dyn std::error::Error>> {
    let tag = Tag::read_from_path(path)?;

    Ok(MusicFileInfo {
        path: path.to_owned(),
        title: tag.title().ok_or("Missing title tag!")?.to_owned(),
        artists: parse_artist_tag(tag.artist().ok_or("Missing artist tag!")?),
    })
}

//Load FLAC meta
fn load_flac_info(path: &str) -> Result<MusicFileInfo, Box<dyn std::error::Error>> {
    let tag = metaflac::Tag::read_from_path(path)?;
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
        artists: artists
    })
}

//Wrapper to write tags by format
pub fn write_tag(discogs: &mut Discogs, config: &TaggerConfig, path: &str, release: &ReleaseMaster, track: &Track) -> Result<(), Box<dyn std::error::Error>> {
    if path.to_ascii_lowercase().ends_with(".mp3") {
        write_mp3_tag(discogs, config, path, release, track)?;
    }
    if path.to_ascii_lowercase().ends_with(".flac") {
        write_flac_tag(discogs, config, path, release, track)?;
    }
    Ok(())
}

fn write_flac_tag(discogs: &mut Discogs, config: &TaggerConfig, path: &str, release: &ReleaseMaster, track: &Track) -> Result<(), Box<dyn std::error::Error>> {
    let mut tag = metaflac::Tag::read_from_path(path)?;
    let vorbis = tag.vorbis_comments_mut();

    //Tags
    if config.title {
        vorbis.set_title(vec![track.title.to_owned()]);
    }
    if config.album {
        vorbis.set_album(vec![release.title.to_owned()]);
    }
    if config.artist {
        vorbis.set_artist(track.artists.as_ref().unwrap_or(release.artists.as_ref().unwrap())
            .into_iter().map(|a| clean_discogs_artist(a)).collect::<Vec<String>>());
    }
    if config.label && release.label.is_some() && release.label.as_ref().unwrap().len() > 0 {
        vorbis.set("LABEL", vec![release.label.as_ref().unwrap().first().unwrap()]);
    }
    if config.date {
        if release.released.is_some() {
            vorbis.set("DATE", vec![release.released.to_owned().unwrap()]);
        } else {
            if release.year.is_some() {
                vorbis.set("DATE", vec![release.year.unwrap().to_string()]);
            }
        }
    }
    if config.genre {
        let mut genres = release.genres.clone();
        let mut styles = release.styles.clone();
        genres.sort();
        styles.sort();
        vorbis.set("GENRE", genres);
        vorbis.set("STYLE", styles);
    }
    if config.track {
        vorbis.set_track(track.position_int as u32);
    }
    //Art
    if config.art && release.art_url.is_some() {
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

fn write_mp3_tag(discogs: &mut Discogs, config: &TaggerConfig, path: &str, release: &ReleaseMaster, track: &Track) -> Result<(), Box<dyn std::error::Error>> {
    let mut tag = Tag::read_from_path(path)?;
    //Tags
    if config.title {
        tag.set_title(&track.title);
    }
    if config.album {
        tag.set_album(&release.title);
    }
    if config.artist {
        tag.set_artist(track.artists.as_ref().unwrap_or(release.artists.as_ref().unwrap())
            .into_iter().map(|a| clean_discogs_artist(a)).collect::<Vec<String>>().join(&config.artist_separator));
    }
    if config.label && release.label.is_some() && release.label.as_ref().unwrap().len() > 0 {
        tag.set_text("TPUB", release.label.as_ref().unwrap().first().unwrap());
    }
    if config.date && release.year.is_some() {
        //Parse date
        let mut month = None;
        let mut day = None;
        if release.released.is_some() && release.released.as_ref().unwrap().len() == 10 {
            match NaiveDate::parse_from_str(release.released.as_ref().unwrap(), "%Y-%m-%d") {
                Ok(d) => {
                    month = Some(d.month() as u8);
                    day = Some(d.day() as u8);
                }
                Err(_) => {}
            }
        }

        tag.set_date_released(Timestamp {
            year: release.year.unwrap() as i32,
            month: month,
            day: day,
            hour: None,
            minute: None,
            second: None
        })
    }
    if config.genre {
        if config.use_styles {
            let mut styles = release.styles.clone();
            styles.sort();
            tag.set_genre(styles.join(", "));
        } else {
            let mut genres = release.genres.clone();
            genres.sort();
            tag.set_genre(genres.join(", "));
        }
    }
    if config.track {
        tag.set_track(track.position_int as u32);
    }
    //Art
    if config.art && release.art_url.is_some() {
        match discogs.download_art(release.art_url.as_ref().unwrap()) {
            Ok(data) => {
                tag.remove_picture_by_type(ID3PictureType::CoverFront);
                tag.add_picture(Picture {
                    mime_type: "image/jpeg".to_string(),
                    picture_type: ID3PictureType::CoverFront,
                    description: "Cover".to_string(),
                    data: data
                });
            },
            Err(_) => eprintln!("Error downloading album art, ignoring!")
        }
    }

    //Save
    tag.write_to_path(path, Version::Id3v24)?;
    Ok(())
}

fn clean_discogs_artist(name: &str) -> String {
    let re = Regex::new(r"\(\d{1,2}\)$").unwrap();
    re.replace(name, "").to_string()
}

//Try to split artist string with common separators
fn parse_artist_tag(src: &str) -> Vec<String> {
    if src.contains(";") {
        return src.split(";").collect::<Vec<&str>>().into_iter().map(|v| v.to_owned()).collect();
    }
    if src.contains(",") {
        return src.split(",").collect::<Vec<&str>>().into_iter().map(|v| v.to_owned()).collect();
    }
    if src.contains("/") {
        return src.split("/").collect::<Vec<&str>>().into_iter().map(|v| v.to_owned()).collect();
    }
    vec![src.to_owned()]
}
