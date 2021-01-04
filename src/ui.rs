extern crate web_view;
extern crate tinyfiledialogs;

use serde_json::Value;
use std::path::Path;
use std::net::TcpListener;
use std::thread;
use tungstenite::server::accept;
use tungstenite::Message;
use std::io::{stdout, Write};
use crossterm::execute;
use crossterm::style::{Color, Print, ResetColor, SetForegroundColor, SetAttribute, Attribute};
use std::time::{SystemTime, Duration};
use std::fs::File;
use webbrowser;

use crate::tagger;
use crate::discogs::Discogs;

pub fn start_ui() {
    //Check if token is saved
    let token = std::fs::read_to_string(".discogstoken").unwrap_or(String::new());

    let content = include_str!("assets/dist.html").replace("###TOKEN###", &token);

    let webview = web_view::builder()
        .invoke_handler(|_, __| Ok(()))
        .content(web_view::Content::Html(content))
        .user_data(())
        .title("Discogs Tagger")
        .size(400, 700)
        .resizable(false)
        .debug(true)
        .build()
        .unwrap();

    //Start socket server
    thread::spawn(|| {
        socket_server();
    });

    webview.run().unwrap();
}

//Create socket server coz webview API uses UI thread
fn socket_server() {
    let server = TcpListener::bind("127.0.0.1:36959").unwrap();
    for stream in server.incoming() {
        thread::spawn (move || {
            let mut websocket = accept(stream.unwrap()).unwrap();
            loop {
                let msg = websocket.read_message().unwrap();
                //Handle only string
                if msg.is_text() {
                    let text = msg.to_text().unwrap();
                    match process_message(text, &mut websocket) {
                        Ok(_) => {},
                        Err(v) => {
                            websocket.write_message(Message::from(format!(r#"{{"action": "alert", "msg": "{}"}}"#, v))).ok();
                        }
                    };
                }
            }
        });
    }
}

//Process websocket messange
fn process_message(text: &str, websocket: &mut tungstenite::WebSocket<std::net::TcpStream>) -> Result<(), String> {
    //Parse JSON
    let json: Value = serde_json::from_str(text).unwrap();
    //Get action
    match json["action"].as_str().unwrap() {
        //Update path
        "browse" => {
            let path = tinyfiledialogs::select_folder_dialog("Select folder", ".");
            if path.is_some() {
                websocket.write_message(Message::from(format!(r#"{{"path": "{}", "action": "path"}}"#, path.unwrap().replace("\\", "\\\\")))).ok();
            }
        },
        //Open external url in browser
        "url" => {
            webbrowser::open(json["url"].as_str().unwrap()).ok();
        },
        "start" => {
            println!("Starting...\n");
            
            let config_data = json["config"].as_object().unwrap();
            //Check path
            let path = config_data["path"].as_str().unwrap();
            if !Path::new(path).is_dir() {
                return Err(String::from("Invalid path!"));
            }
            //Load config
            let config = tagger::TaggerConfig {
                title: config_data["title"].as_bool().unwrap(),
                artist: config_data["artist"].as_bool().unwrap(),
                track: config_data["track"].as_bool().unwrap(),
                album: config_data["album"].as_bool().unwrap(),
                date: config_data["date"].as_bool().unwrap(),
                label: config_data["label"].as_bool().unwrap(),
                artist_separator: String::from(config_data["separator"].as_str().unwrap()),
                fuzziness: config_data["fuzziness"].as_str().unwrap_or("80").parse().unwrap_or(80) as u8,
                art: config_data["art"].as_bool().unwrap(),
                overwrite: config_data["overwrite"].as_bool().unwrap(),
                id3v23: config_data["id3v23"].as_bool().unwrap(),
                id3_genre: config_data["id3Genre"].as_i64().unwrap() as i8,
                flac_genre: config_data["flacGenre"].as_i64().unwrap() as i8,
            };
            //Create discogs
            match Discogs::new() {
                Ok(d) => {
                    let mut discogs = d;
                    //Authorize if token available
                    match config_data["token"].as_str() {
                        Some(v) => {
                            if v.len() > 6 {
                                discogs.authorize_token(String::from(v));
                            }
                        },
                        None => {
                            return Err(String::from("Enter token!"));
                        }
                    }
                    //Set rate limiting
                    discogs.rate_limit(true);
                    //Check token
                    let token_state = discogs.validate_token();
                    if token_state.is_none() {
                        return Err(String::from("Invalid token!"));
                    }
                    //Save token
                    match File::create(".discogstoken") {
                        Ok(mut f) => {
                            f.write_all((&discogs).token.as_ref().unwrap().as_bytes()).ok();
                        },
                        Err(_) => {}
                    };

                    //Toggle button
                    websocket.write_message(Message::from(r#"{"action": "button"}"#)).ok();

                    let mut ok = 0;
                    let mut fail = 0;
                    let ts_start = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap_or(Duration::from_millis(0)).as_secs();

                    //Load files
                    let files = tagger::get_files(path);
                    let total = files.len();
                    for file in files {
                        match tagger::match_track(&mut discogs, &file, config.fuzziness) {
                            Ok(o) => {
                                match o {
                                    Some((track, release)) => {
                                        //Write tag
                                        match tagger::write_tag(&mut discogs, &config, &file.path, &release, &track) {
                                            Ok(_) => {
                                                ok += 1;
                                                print_console(&file.path, Ok(()), ok, fail, total as i32);
                                            },
                                            Err(e) => {
                                                fail += 1;
                                                print_console(&file.path, Err(format!("Failed writing tag to file! {}", e)), ok, fail, total as i32);
                                            }
                                        }
                                        
                                    },
                                    None => {
                                        fail += 1;
                                        print_console(&file.path, Err(String::from("No match!")), ok, fail, total as i32);
                                    }
                                }
                            },
                            Err(e) => {
                                fail += 1;
                                print_console(&file.path, Err(format!("Error matching! {}", e)), ok, fail, total as i32);
                            }
                        }

                        //Update progress in UI
                        let msg = format!(r#"{{"action": "progress", "total": {}, "ok": {}, "fail": {}}}"#, total, ok, fail);
                        websocket.write_message(Message::from(msg)).ok();
                    }
                    //Done
                    print_console_done(ok, fail, total as i32, ts_start);
                    //Toggle button
                    websocket.write_message(Message::from(r#"{"action": "button"}"#)).ok();
                }
                Err(_) => return Err(String::from("Failed initializing Discogs!"))
            };
        },
        _ => {}
    };

    Ok(())
}

//Pretty print done messange
fn print_console_done(ok: i32, fail: i32, total: i32, ts_start: u64) {
    let took = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap_or(Duration::from_millis(0)).as_secs() - ts_start;
    let mut percent = 0;
    if total > 0 {
        percent = ((ok + fail) / total) * 100;
    }

    execute!(
        stdout(),
        SetAttribute(Attribute::Bold),
        Print("\n=============== "),
        SetForegroundColor(Color::Green),
        Print("DONE"),
        ResetColor,
        SetAttribute(Attribute::Bold),
        Print(" ===============\n"),
        SetAttribute(Attribute::Reset),
        Print("Successful: "),
        SetForegroundColor(Color::Green),
        SetAttribute(Attribute::Bold),
        Print(format!("{}\n", ok)),
        SetAttribute(Attribute::Reset),
        ResetColor,
        Print("Failed: "),
        SetForegroundColor(Color::Red),
        SetAttribute(Attribute::Bold),
        Print(format!("{}\n", fail)),
        SetAttribute(Attribute::Reset),
        ResetColor,
        Print("Total: "),
        SetForegroundColor(Color::Blue),
        SetAttribute(Attribute::Bold),
        Print(format!("{}\n", total)),
        SetAttribute(Attribute::Reset),
        ResetColor,
        Print("Success: "),
        SetForegroundColor(Color::Yellow),
        SetAttribute(Attribute::Bold),
        Print(format!("{}%\n", percent)),
        SetAttribute(Attribute::Reset),
        ResetColor,
        Print("Took: "),
        SetForegroundColor(Color::Magenta),
        SetAttribute(Attribute::Bold),
        Print(format!("{:02}:{:02}\n", took / 60, took % 60)),
        SetAttribute(Attribute::Reset),
        ResetColor,
        SetAttribute(Attribute::Bold),
        Print("====================================\n"),
        SetAttribute(Attribute::Reset),
        ResetColor,
    ).ok();
}

//Pretty print in console
fn print_console(path: &str, result: Result<(), String>, ok: i32, fail: i32, total: i32) {
    //Calculate percent
    let mut percent = 0;
    if total > 0 {
        percent = ((ok + fail) * 100) / total;
    }
    //Calculate ETA (remaining *2.1s)
    let eta_s = (total - (ok + fail)) as f64 * 3_f64;
    let eta = format!("{:02}:{:02}", (eta_s / 60_f64) as i32, (eta_s % 60_f64) as i32);

    //Success
    if result.is_ok() {
        execute!(
            stdout(),
            Print("[ "),
            SetForegroundColor(Color::Green),
            SetAttribute(Attribute::Bold),
            Print("OK"),
            ResetColor,
            SetAttribute(Attribute::Reset),
            Print(" ] ["),
            SetAttribute(Attribute::Bold),
            SetForegroundColor(Color::Green),
            Print(format!("{:03}", ok)),
            SetForegroundColor(Color::White),
            Print("/"),
            SetForegroundColor(Color::Red),
            Print(format!("{:03}", fail)),
            SetForegroundColor(Color::White),
            Print("/"),
            SetForegroundColor(Color::Blue),
            Print(format!("{:03}", total - (ok + fail))),
            ResetColor,
            SetAttribute(Attribute::Reset),
            Print("] ["),
            SetForegroundColor(Color::Magenta),
            SetAttribute(Attribute::Bold),
            Print(format!("{}%", percent)),
            SetForegroundColor(Color::White),
            Print(format!(" ETA: {}", eta)),
            ResetColor,
            SetAttribute(Attribute::Reset),
            Print("] "),
            Print(Path::new(path).file_name().unwrap().to_str().unwrap()),
            Print("\n")
        ).ok();
        return;
    }
    //Failed
    execute!(
        stdout(),
        Print("["),
        SetForegroundColor(Color::Red),
        SetAttribute(Attribute::Bold),
        Print("FAIL"),
        ResetColor,
        SetAttribute(Attribute::Reset),
        Print("] ["),
        SetAttribute(Attribute::Bold),
        SetForegroundColor(Color::Green),
        Print(format!("{:03}", ok)),
        SetForegroundColor(Color::White),
        Print("/"),
        SetForegroundColor(Color::Red),
        Print(format!("{:03}", fail)),
        SetForegroundColor(Color::White),
        Print("/"),
        SetForegroundColor(Color::Blue),
        Print(format!("{:03}", total - (ok + fail))),
        ResetColor,
        SetAttribute(Attribute::Reset),
        Print("] ["),
        SetForegroundColor(Color::Magenta),
        SetAttribute(Attribute::Bold),
        Print(format!("{}%", percent)),
        SetForegroundColor(Color::White),
        Print(format!(" ETA: {}", eta)),
        ResetColor,
        SetAttribute(Attribute::Reset),
        Print("] "),
        SetForegroundColor(Color::Red),
        Print(result.unwrap_err()),
        ResetColor,
        Print(format!(" {}", Path::new(path).file_name().unwrap().to_str().unwrap())),
        Print("\n")
    ).ok();
}