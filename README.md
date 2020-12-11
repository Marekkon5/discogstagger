# Discogs Tagger

Simple Rust + Webview app to automatically tag your music collection using data from Discogs.

## How to use

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

BasCurtiz - Request, idea, tester.

## Support

If you wish to support me you can donate at paypal.me/marekkon5