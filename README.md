# ![Logo](https://raw.githubusercontent.com/Marekkon5/discogstagger/main/src/assets/32x32.png) Discogs Tagger

Simple Rust + Webview app to automatically tag your music collection using data from Discogs.

![Screenshot](https://raw.githubusercontent.com/Marekkon5/discogstagger/main/src/assets/screenshot.png)

# Compatibility
<table>
    <thead>
        <tr>
            <th>Tested on platform</th>
            <th>Works correctly</th>
        </tr>
    </thead>
    <tbody>
        <tr>
            <td>Windows 7</td>
            <td>✅</td>
        </tr>
        <tr>
            <td>Windows 10</td>
            <td>✅</td>
        </tr>
        <tr>
            <td>macOS El Capitan</td>
            <td>✅</td>
        </tr>
        <tr>
            <td>macOS Catalina</td>
            <td>✅</td>
        </tr>
        <tr>
            <td>macOS Big Sur</td>
            <td>❌</td>
        </tr>
        <tr>
            <td>Linux</td>
            <td>✅</td>
        </tr>
    </tbody>
</table>

# Troubleshooting

## MacOS:
If you get a warning on macOS, this app can't be opened for whatever reason:  
- Click Apple icon on top left
- Click System Preferences
- Click Security & Privacy
- Click Open Anyway

## Windows:
If you get an error opening the app like: "(Exception from HRESULT: 0x80010007 (RPC_E_SERVER_DIED))"  
- Try to run it without Admin rights.
- In order to do so, make sure <a href="https://articulate.com/support/article/how-to-turn-user-account-control-on-or-off-in-windows-10">UAC</a> is enabled.

# How to use

1. Create account on https://discogs.com  
2. Go to https://www.discogs.com/settings/developers and click `Generate personal token` and copy it.  
3. Download latest version in releases tab (or compile using the instructions below).  
4. Select music folder, paste token, check tags you wanna overwrite and press start!  
5. Tagging might take a long time due to Discogs rate limiting. (~20 tracks / minute)  

## Compiling

Install Rust: https://rustup.rs/

(Optional) Generate HTML, requires NodeJS:
```
npm i -g inline-assets
cd src/assets
inline-assets --htmlmin --cssmin --jsmin index.html dist.html
```

Compile:
```
cargo build --release
```

Then you can also strip (Linux/Mac only) and compress the binary:
```
strip discogstaggerrs
upx -9 discogstaggerrs
```

## Credits

BasCurtiz - Request, idea, tester, trailer.  
Trailer: https://youtu.be/rl5y6NteWk4  
Strictness comparison: https://docs.google.com/spreadsheets/d/1s13-tgcEAF1sete1nBYj9S9eDY1BiZqhcXWevt47s4w/edit?usp=sharing  

## Support

If you wish to support me you can donate at paypal.me/marekkon5