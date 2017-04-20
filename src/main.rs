extern crate bip_bencode;
extern crate docopt;
extern crate hyper;
extern crate itertools;
extern crate ring;
extern crate rustc_serialize;

mod metainfo;
mod tracker;

use bip_bencode::{BencodeRef, BDecodeOpt};
use docopt::Docopt;
use itertools::Itertools;
use metainfo::*;
use ring::rand::SystemRandom;
use std::error::Error;
use std::fs::File;
use std::io::prelude::*;
use std::path::Path;
use tracker::*;

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

    let res = BencodeRef::decode(buf.as_slice(), BDecodeOpt::default())?;

    let info = MetaInfo::new(res)?;
    println!("{:?}", info);

    let rand = SystemRandom::new();
    let peer_id = new_peer_id(&rand)?;
    println!("peer_id: {:x}", peer_id.iter().format(""));

    let tc = TrackerClient::new(&rand, info.clone(), peer_id)?;
    println!("trackerclient: {:?}", tc);

    println!("tracker res: {:?}", tc.easy_start()?);

    Ok(())
}
