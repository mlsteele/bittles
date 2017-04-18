extern crate docopt;
extern crate rustc_serialize;
extern crate bip_bencode;

use std::error::Error;
use std::io::prelude::*;
use std::fs::File;
use std::path::Path;
use docopt::Docopt;
// use bip_bencode::{BencodeRef, BRefAccess, BDecodeOpt};

const USAGE: &'static str = "
Usage: bittles <torrent>
";

#[derive(RustcDecodable)]
struct Args {
    arg_torrent: String,
}

fn main() {
    match inner() {
        Ok(_) => {}
        Err(e) => println!("ERROR: {}", e)
    }
}

fn inner() -> Result<(),Box<Error>> {
    let args: Args = Docopt::new(USAGE)
        .and_then(|d| d.decode())
        .unwrap_or_else(|e| e.exit());

    let cwd = std::env::current_dir()?;
    println!("cwd: {}", cwd.display());
    println!("torrent: {}", args.arg_torrent);

    let torrent_path = Path::new(&args.arg_torrent);
    let mut buf = vec![0;0];
    let mut f = File::open(torrent_path)?;
    f.read_to_end(&mut buf).unwrap();

    for x in buf {
        println!("{}", x);
    }
    Ok(())
}
