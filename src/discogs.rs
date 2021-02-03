use reqwest::blocking::{Client, Response};
use url::form_urlencoded::{Serializer};
use serde_json::{Value};
use reqwest::StatusCode;
use std::time::{SystemTime, Duration};
use std::collections::HashMap;

pub struct Discogs {
    client: Client,
    pub token: Option<String>,
    rate_limit: i16,
    rate_limit_enabled: bool,
    //Timestamp of last request for rate limiting
    last_request: u128,
    //Caches id:value
    release_cache: HashMap<i64, Option<ReleaseMaster>>
}

impl Discogs {
    //New instance
    pub fn new() -> Result<Discogs, Box<dyn std::error::Error>> {
        let client = Client::builder()
            .user_agent("DiscogsTagger/1.0")
            .build()?;
        Ok(Discogs {
            client: client,
            token: None,
            rate_limit: 25,
            rate_limit_enabled: false,
            last_request: 0,
            release_cache: HashMap::new()
        })
    }
    //Authorize with token
    pub fn authorize_token(&mut self, token: String) {
        self.token = Some(token);
        self.rate_limit = 60;
    }
    //Enable/Disable rate limit
    pub fn rate_limit(&mut self, rate_limit: bool) {
        self.rate_limit_enabled = rate_limit;
    }
    //Search random query to check if token valid
    pub fn validate_token(&mut self) -> Option<()> {
        match self.get("https://api.discogs.com/database/search?q=test") {
            Ok(r) => {
                if r.status() == StatusCode::OK {
                    return Some(());
                }
                None
            },
            Err(_) => None
        }
    }
    //Get request wrapper
    fn get(&mut self, url: &str) -> Result<Response, Box<dyn std::error::Error>> {
        //Rate limit delay
        if self.rate_limit_enabled && self.last_request > 0 {
            let now = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap_or(Duration::from_millis(0)).as_millis();
            let diff = now - self.last_request;
            //Rate limit delay in MS
            let mut delay = 1000_f64 / (self.rate_limit as f64 / 60_f64);
            if diff < delay as u128 {
                delay -= diff as f64;
                //Sleep +10ms to prevent rounding/delay errors
                std::thread::sleep(Duration::from_millis((delay + 10_f64) as u64));
            }
        }
        //Create request
        let mut req = self.client.get(url);
        if self.token.is_some() {
            req = req.header("Authorization", format!("Discogs token={}", self.token.as_ref().unwrap()));
        }
        let res = req.send()?;

        //Save request time for rate limiting
        self.last_request = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap_or(Duration::from_millis(0)).as_millis();

        // println!("{:?}", res.headers().get("X-Discogs-Ratelimit-Remaining"));
        Ok(res)
    }

    pub fn download_art(&mut self, url: &str) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        let res = self.get(url)?;
        Ok(res.bytes()?.to_vec())
    }

    pub fn search(&mut self, result_type: Option<&str>, query: Option<&str>, title: Option<&str>, artist: Option<&str>) -> Result<Option<SearchResults>, Box<dyn std::error::Error>> {
        //Generate url
        let mut encoder = Serializer::new(String::new());
        encoder.append_pair("type", result_type.unwrap_or("release,master"));
        if query.is_some() {
            encoder.append_pair("q", query.unwrap());
        }
        if title.is_some() {
            encoder.append_pair("title", title.unwrap());
        }
        if artist.is_some() {
            encoder.append_pair("artist", artist.unwrap());
        }
        let qp = encoder.finish();
        let url = format!("https://api.discogs.com/database/search?{}", qp);
        //Get
        let response = self.get(&url)?;
        //Path
        Ok(SearchResults::from_json(response.json()?))
    }

    pub fn release(&mut self, id: i64) -> Result<Option<ReleaseMaster>, Box<dyn std::error::Error>> {
        //Check if cached
        if self.release_cache.contains_key(&id) {
            return Ok(self.release_cache.get(&id).unwrap().to_owned());
        }
        //Get
        let response = self.get(&format!("https://api.discogs.com/releases/{}", id))?;
        let release = ReleaseMaster::from_json(response.json()?, ReleaseType::Release, None);
        //Cache
        self.release_cache.insert(id, release.clone());
        Ok(release)
    }

    pub fn master(&mut self, id: i64, label: Option<Vec<String>>) -> Result<Option<ReleaseMaster>, Box<dyn std::error::Error>> {
        //Check if cached
        if self.release_cache.contains_key(&id) {
            return Ok(self.release_cache.get(&id).unwrap().to_owned());
        }
        //Get
        let response = self.get(&format!("https://api.discogs.com/masters/{}", id))?;
        let master = ReleaseMaster::from_json(response.json()?, ReleaseType::Master, label);
        //Cache
        self.release_cache.insert(id, master.clone());
        Ok(master)
    }
}

#[derive(Debug, Clone)]
pub struct SearchResults {
    pub releases: Vec<ReleaseMaster>,
    pub masters: Vec<ReleaseMaster>
}

impl SearchResults {
    pub fn from_json(json: Value) -> Option<SearchResults> {
        let mut releases = vec![];
        let mut masters = vec![];
        for result in json["results"].as_array()? {
            //Releases
            if result["type"].as_str().unwrap_or("") == "release" {
                match ReleaseMaster::from_json(result.to_owned(), ReleaseType::Release, None) {
                    Some(r) => releases.push(r),
                    None => {}
                };
            }
            //Masters
            if result["type"].as_str().unwrap_or("") == "master" {
                match ReleaseMaster::from_json(result.to_owned(), ReleaseType::Master, None) {
                    Some(r) => masters.push(r),
                    None => {}
                };
            }
        };

        Some(SearchResults {
            releases,
            masters
        })
    }
}


#[derive(Debug, Clone)]
pub enum ReleaseType {
    Release,
    Master
}

#[derive(Debug, Clone)]
pub struct ReleaseMaster {
    //type is reserved
    pub rtype: ReleaseType,

    pub title: String,
    pub id: i64, 
    pub styles: Vec<String>,
    pub genres: Vec<String>,
    pub url: String, 
    pub country: String,

    pub art_url: Option<String>,
    pub year: Option<i16>,
    pub label: Option<Vec<String>>,
    pub artists: Option<Vec<String>>,
    pub extra_artists: Option<Vec<String>>,
    pub tracks: Option<Vec<Track>>,
    pub released: Option<String>,
}

//Master or release, almost the same
//Masters don't have labels in details, but have in search, option to pass
impl ReleaseMaster {
    pub fn from_json(json: Value, rtype: ReleaseType, label: Option<Vec<String>>) -> Option<ReleaseMaster> {
        Some(ReleaseMaster {
            rtype,
            title: json["title"].as_str()?.to_owned(),
            id: json["id"].as_i64()?,
            //styles, genres, labels are singular in search results json
            styles: json["style"].as_array().unwrap_or(
                json["styles"].as_array().unwrap_or(&vec![])
            ).into_iter().map(|s| s.as_str().unwrap().to_owned()).collect(),
            genres: json["genre"].as_array().unwrap_or(
                json["genres"].as_array().unwrap_or(&vec![])
            ).into_iter().map(|g| g.as_str().unwrap().to_owned()).collect(),
            label: match label {
                Some(l) => Some(l),
                None => {
                    match json["label"].as_array() {
                        Some(l) => Some(l.into_iter().map(|l| l.as_str().unwrap().to_owned()).collect()),
                        None => match json["labels"].as_array() {
                            Some(l) => Some(l.into_iter().map(|l| l["name"].as_str().unwrap().to_owned()).collect()),
                            None => None
                        }
                    }
                }
            },
            url: json["uri"].as_str().unwrap_or("").to_owned(),
            country: json["country"].as_str().unwrap_or("").to_owned(),
            //Available only in full JSON
            artists: match json["artists"].as_array() {
                Some(a) => Some(a.into_iter().map(|a| a["name"].as_str().unwrap().to_owned()).collect()),
                None => None
            },
            extra_artists: match json["extraartists"].as_array() {
                Some(a) => Some(a.into_iter().map(|a| a["name"].as_str().unwrap().to_owned()).collect()),
                None => None
            },
            tracks: match json["tracklist"].as_array() {
                Some(t) => {
                    let mut tracks = vec![];
                    for (i, track) in t.iter().enumerate() {
                        tracks.push(Track::from_json(track.to_owned(), (i as i32)+1));
                    }
                    Some(tracks)
                },
                None => None
            },
            year: match json["year"].as_str() {
                Some(y) => Some(y.parse().unwrap()),
                None => match json["year"].as_i64() {
                    Some(y) => Some(y as i16),
                    None => None
                }
            },
            released: match json["released"].as_str() {
                Some(r) => Some(r.to_owned()),
                None => None
            },
            art_url: match json["cover_image"].as_str() {
                Some(a) => Some(a.to_owned()),
                None => {
                    let empty = vec![];
                    let images = json["images"].as_array().unwrap_or(&empty);
                    match images.first() {
                        Some(i) => Some(i["uri"].as_str().unwrap().to_owned()),
                        None => None
                    }
                }
            }
        })
    }
}

#[derive(Debug, Clone)]
pub struct Track {
    pub title: String,
    pub duration: String,
    pub position: String, 
    pub artists: Option<Vec<String>>,
    pub position_int: i32
}

impl Track {
    pub fn from_json(json: Value, position: i32) -> Track {
        //Artists are available only sometimes
        let artists: Option<Vec<String>> = match json["artists"].as_array() {
            Some(a) => {
                Some(a.into_iter().map(|a| a["name"].as_str().unwrap().to_owned()).collect())
            },
            None => None
        };

        Track {
            title: json["title"].as_str().unwrap().to_owned(),
            duration: json["duration"].as_str().unwrap().to_owned(),
            position: json["position"].as_str().unwrap().to_owned(),
            position_int: json["position"].as_str().unwrap().parse().unwrap_or(position),
            artists: artists
        }
    }
}